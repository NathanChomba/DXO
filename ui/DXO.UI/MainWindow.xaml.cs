using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Linq;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using Microsoft.UI.Composition;
using Microsoft.UI.Xaml.Hosting;
using System.Numerics;
using DXO.UI.Services;
using DXO.UI.ViewModels;
using DXO.UI.Views;
using Microsoft.UI.Windowing;

namespace DXO.UI;

public sealed partial class MainWindow : Window
{
    public DeviceViewModel Device => App.Device;

    private TrayIcon? _tray;
    private bool _exiting;

    public MainWindow()
    {
        InitializeComponent();
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        AppWindow.Resize(new Windows.Graphics.SizeInt32(1280, 800));

        string icon = System.IO.Path.Combine(AppContext.BaseDirectory, "dxo.ico");
        if (System.IO.File.Exists(icon)) AppWindow.SetIcon(icon);
        string logo = System.IO.Path.Combine(AppContext.BaseDirectory, "dxo-logo.png");
        if (System.IO.File.Exists(logo)) TitleLogo.Source = new Microsoft.UI.Xaml.Media.Imaging.BitmapImage(new Uri(logo));

        try
        {
            _tray = new TrayIcon(WinRT.Interop.WindowNative.GetWindowHandle(this), icon, "DXO");
            _tray.OpenRequested += ShowFromTray;
            _tray.ExitRequested += ExitApp;
        }
        catch (Exception ex) { App.LogCrash("Tray", ex); } // no tray: closing just closes

        AppWindow.Closing += OnClosing;
        AppWindow.Changed += OnWindowChanged;
        Closed += (_, _) => _tray?.Dispose();
    }

    // ================= tray =================

    private void OnClosing(AppWindow sender, AppWindowClosingEventArgs args)
    {
        if (_exiting || _tray is null || !App.Settings.CloseToTray) return;
        args.Cancel = true;
        HideToTray();
    }

    private void OnWindowChanged(AppWindow sender, AppWindowChangedEventArgs args)
    {
        if (_tray is not null && App.Settings.MinimizeToTray && sender.IsVisible &&
            sender.Presenter is OverlappedPresenter { State: OverlappedPresenterState.Minimized })
            HideToTray();
    }

    private void HideToTray()
    {
        AppWindow.Hide();
        if (!App.Settings.TrayHintShown)
        {
            _tray?.ShowBalloon("DXO is still running", "Your controller keeps working. Click the DXO icon to open it, or right-click to exit.");
            App.Settings.TrayHintShown = true;
            App.Settings.Save();
        }
    }

    public void ShowFromTray()
    {
        if (AppWindow.Presenter is OverlappedPresenter { State: OverlappedPresenterState.Minimized } p) p.Restore();
        AppWindow.Show();
        Activate();
    }

    /// Really quit: close the window for good (App disposes the engine, which unhides the pad).
    public void ExitApp()
    {
        _exiting = true;
        Close();
    }

    public void SetTrayTip(string tip) => _tray?.SetTip(tip);

    /// Called by App once App.Device exists, so x:Bind sees a non-null source.
    public void Start()
    {
        Bindings.Update();
        Device.PropertyChanged += OnDeviceChanged;
        Device.PadsChanged += OnPadsChanged;
        Nav.SelectedItem = Nav.MenuItems[0];
    }

    private void OnDeviceChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(DeviceViewModel.Connected) or nameof(DeviceViewModel.DeviceName) or nameof(DeviceViewModel.ProfileName))
            SetTrayTip(Device.Connected ? $"DXO: {Device.DeviceName} ({Device.ProfileName})" : "DXO: no controller");
    }

    // ================= controller bar + card =================

    private PadViewModel? _cardPad;
    private FrameworkElement? _cardPill;
    private Storyboard? _cardAnim;
    private bool _cardOpen, _cardLoading;
    private const double CardW = 440, CardH = 330;

    /// The selected controller changed (or one came or went): pages show
    /// per-controller data, so reload the one on screen.
    private int _shownSlot = -1;

    private void OnPadsChanged()
    {
        if (_cardOpen && _cardPad is not null && !Device.Pads.Contains(_cardPad)) CloseCard(); // unplugged
        // Only reload when the controller the pages show actually changed.
        int slot = App.Engine?.SelectedPad ?? -1;
        if (slot == _shownSlot) return;
        _shownSlot = slot;
        ReloadPage();
    }

    private void ReloadPage()
    {
        if (ContentFrame.CurrentSourcePageType is Type t && t != typeof(HomePage) && t != typeof(SettingsPage) &&
            t != typeof(AboutPage) && t != typeof(GamesPage) && t != typeof(ProfilesPage))
        {
            NavigateFrame(t);
            ContentFrame.BackStack.Clear();
        }
    }

    private void Pill_Click(object sender, RoutedEventArgs e)
    {
        if (sender is not FrameworkElement pill || pill.DataContext is not PadViewModel pad) return;
        if (_cardOpen && _cardPad == pad) { CloseCard(); return; }
        Device.SelectPad(pad.Slot); // pages now follow this controller
        OpenCard(pad, pill);
    }

    private void Pair_Click(object sender, RoutedEventArgs e)
    {
        try { Process.Start(new ProcessStartInfo("ms-settings:bluetooth") { UseShellExecute = true }); }
        catch (Exception ex) { App.LogCrash("Open Bluetooth settings", ex); }
    }

    private void FillCard(PadViewModel pad)
    {
        _cardLoading = true;
        CardName.Text = pad.Name;
        CardPlayer.Text = pad.PlayerText;
        CardBattery.Text = pad.BatteryText;
        CardLinkIcon.Glyph = pad.TransportGlyph;
        CardLink.Text = pad.TransportText;
        CardRate.Text = pad.RateText;
        CardTarget.Text = pad.GamesSee;
        CardHide.IsOn = pad.Hidden;
        CardHide.IsEnabled = pad.Target != 0; // hiding only matters with a virtual pad
        var names = ProfileStore.All().Select(p => p.Model.Name).ToList();
        if (!names.Contains(pad.Profile)) names.Insert(0, pad.Profile);
        CardProfile.ItemsSource = names;
        CardProfile.SelectedItem = pad.Profile;
        CardReconnectText.Text = "Reconnect";
        CardReconnect.IsEnabled = true;
        _cardLoading = false;
    }

    private RectangleClip? _clip;
    private Rect4 _pillRect;   // the pill's box inside the card, where the reveal starts and ends

    private readonly record struct Rect4(float L, float T, float R, float B);

    /// Pill (32 px tall) and card share one corner radius: Windows can animate a
    /// clip's edges but not its corner rounding.
    private const float CornerR = 16;

    private void OpenCard(PadViewModel pad, FrameworkElement pill)
    {
        _cardPad = pad;
        _cardPill = pill;
        FillCard(pad);

        // Put the card's top-left on the pill, nudged left if it would run off the window.
        var at = pill.TransformToVisual(Root).TransformPoint(new Windows.Foundation.Point(0, 0));
        double left = Math.Max(8, Math.Min(at.X, Root.ActualWidth - CardW - 8));
        Canvas.SetLeft(Island, left);
        Canvas.SetTop(Island, at.Y);
        float offX = (float)(at.X - left);
        _pillRect = new Rect4(offX, 0, offX + (float)pill.ActualWidth, (float)pill.ActualHeight);

        // Clip down to the pill BEFORE showing the layer, so the first frame is the pill.
        var visual = ElementCompositionPreview.GetElementVisual(Island);
        _clip ??= visual.Compositor.CreateRectangleClip();
        visual.Clip = _clip;
        SetClip(_pillRect, CornerR);
        IslandContent.Opacity = 0;
        IslandScrim.Opacity = 0;

        IslandLayer.Visibility = Visibility.Visible;
        _cardOpen = true;
        Animate(open: true);
        CardProfile.Focus(FocusState.Programmatic);
    }

    private void CloseCard()
    {
        if (!_cardOpen) return;
        _cardOpen = false;
        Animate(open: false);
    }

    private void SetClip(Rect4 r, float radius)
    {
        if (_clip is null) return;
        (_clip.Left, _clip.Top, _clip.Right, _clip.Bottom) = (r.L, r.T, r.R, r.B);
        var v = new Vector2(radius, radius);
        _clip.TopLeftRadius = _clip.TopRightRadius = _clip.BottomRightRadius = _clip.BottomLeftRadius = v;
    }

    /// Opening: a rounded window grows from the pill's exact shape to the full
    /// card (the card itself never stretches, so text stays crisp), then the
    /// contents fade up. Closing: contents go first, then it shrinks back.
    private void Animate(bool open)
    {
        if (_clip is null) return;
        var comp = _clip.Compositor;
        var full = new Rect4(0, 0, (float)CardW, (float)CardH);
        var (from, to) = open ? (_pillRect, full) : (full, _pillRect);
        var ease = open
            ? comp.CreateCubicBezierEasingFunction(new Vector2(0.16f, 1f), new Vector2(0.3f, 1f))   // fast start, soft landing
            : comp.CreateCubicBezierEasingFunction(new Vector2(0.5f, 0f), new Vector2(0.75f, 0.2f)); // gentle pull-in
        double kWin = App.Settings.WindowAnimationScale, kFade = App.Settings.AnimatorDurationScale;
        if (kWin <= 0)
        {
            // Animations off: jump straight to the end state.
            SetClip(to, CornerR);
            IslandScrim.Opacity = IslandContent.Opacity = open ? 1 : 0;
            if (!open) IslandLayer.Visibility = Visibility.Collapsed;
            return;
        }
        var dur = TimeSpan.FromMilliseconds(Math.Max(1, (open ? 460 : 240) * kWin));
        var delay = TimeSpan.FromMilliseconds((open ? 0 : 70) * kWin);

        ScalarKeyFrameAnimation Scalar(float a, float b)
        {
            var an = comp.CreateScalarKeyFrameAnimation();
            an.InsertKeyFrame(0f, a);
            an.InsertKeyFrame(1f, b, ease);
            an.Duration = dur;
            an.DelayTime = delay;
            an.DelayBehavior = AnimationDelayBehavior.SetInitialValueBeforeDelay;
            return an;
        }

        var batch = comp.CreateScopedBatch(CompositionBatchTypes.Animation);
        _clip.StartAnimation("Left", Scalar(from.L, to.L));
        _clip.StartAnimation("Top", Scalar(from.T, to.T));
        _clip.StartAnimation("Right", Scalar(from.R, to.R));
        _clip.StartAnimation("Bottom", Scalar(from.B, to.B));
        batch.End();
        batch.Completed += (_, _) =>
        {
            if (!_cardOpen) DispatcherQueue.TryEnqueue(() => { if (!_cardOpen) IslandLayer.Visibility = Visibility.Collapsed; });
        };

        // Scrim and contents: plain XAML opacity fades.
        _cardAnim?.Stop();
        var sb = new Storyboard();
        void Fade(UIElement target, double to, int ms, int beginMs)
        {
            if (kFade <= 0) { target.Opacity = to; return; }
            var a = new DoubleAnimation
            {
                // Length follows the animator scale; the start waits for the card (window scale).
                From = target.Opacity, To = to, Duration = TimeSpan.FromMilliseconds(Math.Max(1, ms * kFade)),
                BeginTime = TimeSpan.FromMilliseconds(beginMs * kWin), EasingFunction = new CubicEase { EasingMode = EasingMode.EaseOut },
            };
            Storyboard.SetTarget(a, target);
            Storyboard.SetTargetProperty(a, "Opacity");
            sb.Children.Add(a);
        }
        Fade(IslandScrim, open ? 1 : 0, open ? 280 : 220, 0);
        Fade(IslandContent, open ? 1 : 0, open ? 220 : 80, open ? 200 : 0);
        _cardAnim = sb;
        sb.Begin();
    }

    private void PadScroll_PointerWheelChanged(object sender, PointerRoutedEventArgs e)
    {
        // A normal mouse wheel scrolls the controller bar sideways.
        int delta = e.GetCurrentPoint(PadScroll).Properties.MouseWheelDelta;
        if (PadScroll.ScrollableWidth <= 0) return;
        PadScroll.ChangeView(PadScroll.HorizontalOffset - delta, null, null);
        e.Handled = true;
    }

    private void Scrim_Tapped(object sender, TappedRoutedEventArgs e) => CloseCard();

    private void Root_KeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == Windows.System.VirtualKey.Escape && _cardOpen) { CloseCard(); e.Handled = true; }
    }

    private void CardHide_Toggled(object sender, RoutedEventArgs e)
    {
        if (_cardLoading || _cardPad is null || App.Engine is not { } eng) return;
        Device.SelectPad(_cardPad.Slot);
        var v = eng.GetVirtual();
        eng.SetVirtual(v.Target, CardHide.IsOn, v.RumblePassthrough != 0); // reconnects that pad with the new setup
    }

    private void CardProfile_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_cardLoading || _cardPad is null || CardProfile.SelectedItem is not string name || name == _cardPad.Profile) return;
        var entry = ProfileStore.All().FirstOrDefault(p => p.Model.Name == name);
        if (entry.Model is null) return;
        Device.SelectPad(_cardPad.Slot);
        if (App.Engine?.SetActiveProfileJson(entry.Model.ToJson()) == true)
        {
            _cardPad.Profile = name;
            ReloadPage(); // the page on screen shows this pad's settings
        }
    }

    private async void CardReconnect_Click(object sender, RoutedEventArgs e)
    {
        if (_cardPad is null) return;
        Device.SelectPad(_cardPad.Slot);
        App.Engine?.Reconnect();
        CardReconnect.IsEnabled = false;
        CardReconnectText.Text = "Reconnecting...";
        await System.Threading.Tasks.Task.Delay(2500); // unhide, restart, hide again
        CardReconnectText.Text = "Reconnect";
        CardReconnect.IsEnabled = true;
    }

    private void CardOpen_Click(object sender, RoutedEventArgs e)
    {
        if (_cardPad is not null) Device.SelectPad(_cardPad.Slot);
        CloseCard();
        Select("controller");
        if (ContentFrame.CurrentSourcePageType == typeof(MyControllerPage)) ReloadPage(); // already there: refresh
    }

    /// Lets pages jump to another page by nav tag, e.g. Home's quick-action tiles.
    public static void NavigateTo(string tag) =>
        ((MainWindow?)((App)Application.Current).MainWindowRef)?.Select(tag);

    public void OpenSettings() => Nav.SelectedItem = Nav.SettingsItem;

    private void Select(string tag)
    {
        foreach (var item in Nav.MenuItems)
            if (item is NavigationViewItem n && (string)n.Tag == tag) { Nav.SelectedItem = n; return; }
    }

    private void Nav_SelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (args.IsSettingsSelected) { Go(typeof(SettingsPage)); return; }

        switch (args.SelectedItemContainer?.Tag as string)
        {
            case "home": Go(typeof(HomePage)); break;
            case "controller": Go(typeof(MyControllerPage)); break;
            case "profiles": Go(typeof(ProfilesPage)); break;
            case "virtual": Go(typeof(VirtualDevicePage)); break;
            case "triggers": Go(typeof(TriggersPage)); break;
            case "lighting": Go(typeof(LightingPage)); break;
            case "mappings": Go(typeof(MappingsPage)); break;
            case "diagnostics": Go(typeof(DiagnosticsPage)); break;
            case "haptics": Go(typeof(HapticsPage)); break;
            case "motion": Go(typeof(MotionPage)); break;
            case "games": Go(typeof(GamesPage)); break;
            case "about": Go(typeof(AboutPage)); break;
        }
    }

    private void Go(Type page)
    {
        if (ContentFrame.CurrentSourcePageType != page)
            NavigateFrame(page);
    }

    /// Page change with DXO's own entrance (fade + short slide up), scaled by
    /// Settings > Animations > Transition animation scale. The built-in Windows
    /// transition can't be sped up, so it's switched off.
    private void NavigateFrame(Type page, object? parameter = null)
    {
        ContentFrame.Navigate(page, parameter, new SuppressNavigationTransitionInfo());
        double k = App.Settings.TransitionAnimationScale;
        if (k <= 0 || ContentFrame.Content is not UIElement el) return;

        var slide = new TranslateTransform { Y = 14 };
        el.RenderTransform = slide;
        el.Opacity = 0;
        var sb = new Storyboard();
        var fade = new DoubleAnimation
        {
            From = 0, To = 1, Duration = TimeSpan.FromMilliseconds(240 * k),
            EasingFunction = new CubicEase { EasingMode = EasingMode.EaseOut },
        };
        Storyboard.SetTarget(fade, el);
        Storyboard.SetTargetProperty(fade, "Opacity");
        var move = new DoubleAnimation
        {
            From = 14, To = 0, Duration = TimeSpan.FromMilliseconds(320 * k),
            EasingFunction = new ExponentialEase { EasingMode = EasingMode.EaseOut, Exponent = 5 },
        };
        Storyboard.SetTarget(move, slide);
        Storyboard.SetTargetProperty(move, "Y");
        sb.Children.Add(fade);
        sb.Children.Add(move);
        sb.Begin();
    }

    private void Soon(string title, string what, string when) =>
        NavigateFrame(typeof(PlaceholderPage), new PlaceholderInfo(title, what, when));
}
