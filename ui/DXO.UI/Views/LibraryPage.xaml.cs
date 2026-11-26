using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.Media.Core;
using Windows.Media.Playback;
using DXO.UI.Services;

namespace DXO.UI.Views;

/// One tile in the row.
public sealed class LibraryItem : INotifyPropertyChanged
{
    public LibraryGame Game { get; }
    public string Name => Game.Name;
    public string Initials { get; private set; }
    public ImageSource? Cover { get; private set; }
    public Visibility StarVisibility => Game.Favourite ? Visibility.Visible : Visibility.Collapsed;
    /// Tile size from Settings (box-art shape, 2:3).
    public double TileWidth => LibraryPage.TileWidth;
    public double TileHeight => LibraryPage.TileHeight;

    private bool _highlighted;
    /// The highlighted tile shows its name.
    public bool Highlighted
    {
        get => _highlighted;
        set
        {
            if (_highlighted == value) return;
            _highlighted = value;
            PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(NameVisibility)));
        }
    }
    public Visibility NameVisibility => _highlighted ? Visibility.Visible : Visibility.Collapsed;

    public event PropertyChangedEventHandler? PropertyChanged;

    public LibraryItem(LibraryGame game)
    {
        Game = game;
        Cover = LibraryPage.LoadPicture(game.Artwork, 300);
        // With a cover there is nothing to show behind it; without one, the game's initials.
        Initials = Cover is null ? LibraryPage.InitialsOf(game.Name) : "";
    }

    /// The game got a new cover (or name): redraw just this tile.
    public void Refresh()
    {
        Cover = LibraryPage.LoadPicture(Game.Artwork, 300);
        Initials = Cover is null ? LibraryPage.InitialsOf(Game.Name) : "";
        foreach (string p in new[] { nameof(Cover), nameof(Initials), nameof(Name), nameof(StarVisibility) })
            PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(p));
    }
}

/// Milestone 7.1: the Games page, console style.
///
/// Library: the highlighted game's artwork fills the page (fading from one
/// game to the next); one row of square tiles at the bottom slides along as
/// the highlight moves, most recently played first; round icons at the top
/// search, filter, scan and add. Hovering a tile with the mouse highlights it.
///
/// Details: one click (or Cross / A, or Options / Menu) opens the game's own
/// screen, laid out like a console game card: cover and facts, the wide
/// picture or trailer, the description, and Play / achievements / launcher /
/// game details along the bottom. Circle / B goes back.
public sealed partial class LibraryPage : Page
{
    private const string AllGames = "All games", Favourites = "Favourites";
    /// How much larger the highlighted tile is drawn.
    private const double Zoom = 1.14;
    private const int SlideMs = 180, FadeMs = 420;
    /// Tile size: Settings > Games page > Tile size scales the normal 140 x 210.
    public static double TileScale => Math.Clamp(App.Settings.GameTileScale, 0.75, 1.5);
    public static double TileWidth => Math.Round(140 * TileScale);
    public static double TileHeight => Math.Round(210 * TileScale);
    /// How long each backdrop stays in a slideshow.
    private static readonly TimeSpan SlideshowStep = TimeSpan.FromSeconds(12);

    private List<LibraryGame> _games = new();
    private LibraryGame? _selected;
    private string _query = "", _filter = AllGames;
    private bool _loading, _started, _filling, _fromHover;
    private Border _front;
    private string _gifShown = "";
    private string _backdropShown = "\0";
    /// The trailer panel's own player, kept only so its failure handler is hooked once.
    private MediaPlayer? _player;
    private LibraryGame? _infoFor;
    private string _status = "";
    private Microsoft.UI.Dispatching.DispatcherQueueTimer? _statusTimer;
    private ScrollViewer? _rowScroller;
    /// Where the mouse was the last time hovering picked a tile. A tile that
    /// slides under a mouse that hasn't moved (the row scrolling) is not a hover:
    /// treating it as one made the row chase the mouse and shake side to side.
    private Windows.Foundation.Point? _hoverAt;
    private Windows.Foundation.Point? _lastPointer;
    private MediaPlayer? _bgPlayer;
    private string _bgVideo = "";
    private Microsoft.UI.Dispatching.DispatcherQueueTimer? _videoDelay, _slideshow;
    private int _slideIndex;
    private readonly CancellationTokenSource _leaving = new();

    public LibraryPage()
    {
        InitializeComponent();
        _front = BackdropA;
        // The row is as tall as the tiles plus room for the highlighted one to grow.
        GameGrid.Height = TileHeight * Zoom + 32;
        if (App.Settings.LivingBackdrop) StartLivingBackdrop();
        _videoDelay = DispatcherQueue.CreateTimer();
        _videoDelay.Interval = TimeSpan.FromMilliseconds(450); // don't start a video for every tile passed over
        _videoDelay.IsRepeating = false;
        _videoDelay.Tick += (_, _) => StartBackdropVideo();
        _slideshow = DispatcherQueue.CreateTimer();
        _slideshow.Interval = SlideshowStep;
        _slideshow.Tick += (_, _) => NextSlide();
        if (App.Settings.BackdropMode == "Slideshow") _slideshow.Start();
        Loaded += OnLoaded;
        LibraryView.SizeChanged += (_, e) => SizeBackdrop(e.NewSize);
        // Track the mouse across the whole page, including over the details screen.
        AddHandler(PointerMovedEvent, new PointerEventHandler((_, e) => _lastPointer = e.GetCurrentPoint(null).Position), handledEventsToo: true);
        // Keep the name under its tile while the row slides or the window is resized.
        InfoLayer.SizeChanged += (_, _) => PlaceInfo();
        GameGrid.Loaded += (_, _) =>
        {
            _rowScroller ??= FindScroller(GameGrid);
            if (_rowScroller is not null) _rowScroller.ViewChanged += (_, _) => PlaceInfo();
        };
        Unloaded += (_, _) =>
        {
            _leaving.Cancel(); // stop looking things up when the page goes away
            StopTrailer();
            _slideshow?.Stop();
            _videoDelay?.Stop();
            StopBackdropVideo();
        };

        // Moving the highlight with the pad or the arrow keys picks the game, so the
        // backdrop follows without a button press.
        GameGrid.GotFocus += (_, e) =>
        {
            if (e.OriginalSource is GridViewItem c && GameGrid.ItemFromContainer(c) is LibraryItem it
                && !ReferenceEquals(GameGrid.SelectedItem, it))
                GameGrid.SelectedItem = it;
        };
        // Tiles are created as they scroll into view: size them, and let the mouse highlight them.
        GameGrid.ContainerContentChanging += (_, a) =>
        {
            if (a.ItemContainer is not GridViewItem c) return;
            SetZoom(c, ReferenceEquals((a.Item as LibraryItem)?.Game, _selected), animate: false);
            if (c.Tag as string == "hooked") return; // containers are reused: hook each once
            c.Tag = "hooked";
            c.PointerEntered += (_, e) =>
            {
                if (DetailsView.Visibility == Visibility.Visible) return;
                var at = e.GetCurrentPoint(null).Position;
                if (_hoverAt is { } was && Math.Abs(at.X - was.X) < 3 && Math.Abs(at.Y - was.Y) < 3) return; // mouse didn't move
                _hoverAt = at;
                if (GameGrid.ItemFromContainer(c) is not LibraryItem it || ReferenceEquals(GameGrid.SelectedItem, it)) return;
                _fromHover = true; // the tile is already under the mouse: don't slide the row
                GameGrid.SelectedItem = it;
                _fromHover = false;
            };
        };
    }

    private async void OnLoaded(object sender, RoutedEventArgs e)
    {
        if (_started) return;
        _started = true;
        try
        {
            _games = GameLibrary.Load();
            ShowGames();
            if (_games.Count == 0) await ScanAsync(); // first visit: look for installed games
            await FillMissingAsync();
        }
        catch (Exception ex) { App.LogCrash("LibraryPage", ex); }
    }

    // ================= pictures and animation =================

    public static string InitialsOf(string name)
    {
        var words = name.Split(' ', StringSplitOptions.RemoveEmptyEntries);
        string s = string.Concat(words.Take(2).Select(w => char.ToUpperInvariant(w[0])));
        return s.Length > 0 ? s : "?";
    }

    /// A picture from DXO's art folder. `decodeWidth` loads it smaller to save
    /// memory (a tiny width also gives a soft, blurred look when stretched).
    public static ImageSource? LoadPicture(string path, int decodeWidth = 0)
    {
        try
        {
            if (path.Length == 0 || !File.Exists(path)) return null;
            var bmp = new BitmapImage();
            if (decodeWidth > 0) bmp.DecodePixelWidth = decodeWidth;
            bmp.UriSource = new Uri(path);
            return bmp;
        }
        catch { return null; /* a broken picture just means no picture */ }
    }

    private static void Animate(DependencyObject target, string property, double to, int ms)
    {
        var anim = new DoubleAnimation
        {
            To = to,
            Duration = TimeSpan.FromMilliseconds(ms),
            EasingFunction = new QuadraticEase { EasingMode = EasingMode.EaseOut },
        };
        Storyboard.SetTarget(anim, target);
        Storyboard.SetTargetProperty(anim, property);
        var sb = new Storyboard();
        sb.Children.Add(anim);
        sb.Begin();
    }

    // ================= backdrop: living, animated, several =================

    /// The slow zoom and drift that makes the artwork feel alive: zoom in a
    /// little while sliding sideways, then back, over about half a minute.
    private void StartLivingBackdrop()
    {
        var sb = new Storyboard { RepeatBehavior = RepeatBehavior.Forever, AutoReverse = true };
        void Add(string property, double from, double to)
        {
            var a = new DoubleAnimation
            {
                From = from,
                To = to,
                Duration = TimeSpan.FromSeconds(26),
                EasingFunction = new SineEase { EasingMode = EasingMode.EaseInOut },
            };
            Storyboard.SetTarget(a, BackdropMotion);
            Storyboard.SetTargetProperty(a, property);
            sb.Children.Add(a);
        }
        Add("ScaleX", 1.02, 1.10);
        Add("ScaleY", 1.02, 1.10);
        Add("TranslateX", -24, 24);
        Add("TranslateY", 8, -8);
        sb.Begin();
    }

    /// Show the highlighted game's backdrop: its animated background if it has
    /// one (and Settings allows), else one of its pictures (the chosen one, or
    /// the next in the slideshow), else its cover enlarged and dimmed.
    private void ShowGameBackdrop(bool resetSlideshow)
    {
        if (resetSlideshow)
        {
            _slideIndex = 0;
            _slideshow?.Stop();
            if (App.Settings.BackdropMode == "Slideshow") _slideshow?.Start(); // a full step on each new game
        }
        _videoDelay?.Stop();
        var g = _selected;
        string anim = g is not null && App.Settings.AnimatedBackdrops && g.AnimatedBackground.Length > 0 && File.Exists(g.AnimatedBackground)
            ? g.AnimatedBackground : "";
        if (anim.Length == 0 || !GameLibrary.IsVideo(anim)) StopBackdropVideo();
        ShowGif(anim.Length > 0 && !GameLibrary.IsVideo(anim) ? anim : "");

        if (g is null) { ShowBackdrop("", 0); return; }
        if (anim.Length > 0 && !GameLibrary.IsVideo(anim)) return; // the GIF is showing, over the picture

        var pictures = GameLibrary.AllBackdrops(g);
        if (pictures.Count > 0)
        {
            string pick = App.Settings.BackdropMode == "Slideshow" ? pictures[_slideIndex % pictures.Count] : pictures[0];
            ShowBackdrop(pick, 0.9);
        }
        else if (g.Artwork.Length > 0 && File.Exists(g.Artwork)) ShowBackdrop(g.Artwork, 0.35); // the cover, enlarged and dimmed
        else ShowBackdrop("", 0);

        // A video starts once the highlight has settled, over the still picture.
        if (anim.Length > 0 && anim != _bgVideo) _videoDelay?.Start();
    }

    /// Show a GIF over the backdrop pictures, or hide it ("").
    private void ShowGif(string path)
    {
        if (path == _gifShown) return;
        _gifShown = path;
        if (path.Length == 0)
        {
            Animate(BackdropGif, "Opacity", 0, FadeMs);
            BackdropGif.Source = null;
            return;
        }
        BackdropGif.Source = LoadPicture(path); // full size: a GIF won't animate if decoded smaller
        Animate(BackdropGif, "Opacity", 0.9, FadeMs);
    }

    /// The animated layers are given the page's size here, so they fill it
    /// without ever asking for more room. The page also clips them at its edges.
    private void SizeBackdrop(Windows.Foundation.Size size)
    {
        BackdropHost.Width = size.Width;
        BackdropHost.Height = size.Height;
        BackdropGif.Width = BackdropVideo.Width = size.Width;
        BackdropGif.Height = BackdropVideo.Height = size.Height;
        LibraryView.Clip = new RectangleGeometry { Rect = new Windows.Foundation.Rect(0, 0, size.Width, size.Height) };
    }

    private void NextSlide()
    {
        if (_selected is null || DetailsView.Visibility == Visibility.Visible || _bgVideo.Length > 0) return;
        if (GameLibrary.AllBackdrops(_selected).Count < 2) return;
        _slideIndex++;
        ShowGameBackdrop(resetSlideshow: false);
    }

    private void StartBackdropVideo()
    {
        var g = _selected;
        if (g is null || !App.Settings.AnimatedBackdrops) return;
        string path = g.AnimatedBackground;
        if (path.Length == 0 || !File.Exists(path) || !GameLibrary.IsVideo(path) || path == _bgVideo) return;
        try
        {
            if (_bgPlayer is null)
            {
                // One player for the page's life, never thrown away while shown (that crashes WinUI).
                _bgPlayer = new MediaPlayer { IsMuted = true, IsLoopingEnabled = true, AutoPlay = true };
                _bgPlayer.MediaFailed += (_, _) => DispatcherQueue.TryEnqueue(StopBackdropVideo);
                BackdropVideo.SetMediaPlayer(_bgPlayer);
            }
            _bgVideo = path;
            _bgPlayer.Source = MediaSource.CreateFromUri(new Uri(path));
            Animate(BackdropVideo, "Opacity", 0.9, FadeMs);
        }
        catch (Exception ex)
        {
            App.LogCrash("LibraryPage.BackdropVideo", ex);
            StopBackdropVideo();
        }
    }

    private void StopBackdropVideo()
    {
        if (_bgVideo.Length == 0) return;
        _bgVideo = "";
        BackdropVideo.Opacity = 0;
        try
        {
            _bgPlayer?.Pause();
            if (_bgPlayer is not null) _bgPlayer.Source = null;
        }
        catch (Exception ex) { App.LogCrash("LibraryPage.StopBackdropVideo", ex); }
    }

    /// Fade the page's backdrop over to a new picture.
    private void ShowBackdrop(string path, double opacity)
    {
        string key = path + "|" + opacity;
        if (key == _backdropShown) return;
        _backdropShown = key;
        var next = ReferenceEquals(_front, BackdropA) ? BackdropB : BackdropA;
        var brush = ReferenceEquals(next, BackdropA) ? BackdropABrush : BackdropBBrush;
        brush.ImageSource = LoadPicture(path, 1920);
        // No reordering: one picture fading in while the other fades out looks the same
        // either way round, and raising a layer here once covered the whole page.
        Animate(next, "Opacity", brush.ImageSource is null ? 0 : opacity, FadeMs);
        Animate(_front, "Opacity", 0, FadeMs);
        _front = next;
    }

    /// Draw a tile larger without moving its neighbours: it grows upwards
    /// from its bottom edge, over the row's top padding.
    private static void SetZoom(GridViewItem tile, bool on, bool animate)
    {
        if (tile.RenderTransform is not ScaleTransform scale)
        {
            scale = new ScaleTransform { ScaleX = 1, ScaleY = 1 };
            tile.RenderTransform = scale;
            tile.RenderTransformOrigin = new Windows.Foundation.Point(0.5, 1);
        }
        double to = on ? Zoom : 1;
        Canvas.SetZIndex(tile, on ? 1 : 0);
        if (!animate)
        {
            scale.ScaleX = scale.ScaleY = to;
            return;
        }
        Animate(scale, "ScaleX", to, SlideMs);
        Animate(scale, "ScaleY", to, SlideMs);
    }

    // ================= the row =================

    private void ShowGames()
    {
        var shown = _games.Where(g => !g.Hidden)
            .Where(g => _query.Length == 0 || g.Name.Contains(_query, StringComparison.CurrentCultureIgnoreCase))
            .Where(g => _filter == AllGames || (_filter == Favourites ? g.Favourite : HasGenre(g, _filter)))
            // Most recently played first; games never played follow, A to Z.
            .OrderByDescending(g => g.LastPlayed ?? DateTime.MinValue)
            .ThenBy(g => g.Name, StringComparer.CurrentCultureIgnoreCase)
            .Select(g => new LibraryItem(g)).ToList();

        // Keep the highlighted game if it is still shown; otherwise highlight the first.
        var pick = shown.FirstOrDefault(i => ReferenceEquals(i.Game, _selected)) ?? shown.FirstOrDefault();
        _selected = pick?.Game;
        double offset = _rowScroller?.HorizontalOffset ?? 0;
        _loading = true;
        GameGrid.ItemsSource = shown;
        GameGrid.SelectedItem = pick;
        _loading = false;
        // A new list starts scrolled to the beginning: put the row back where it was.
        if (offset > 0) _rowScroller?.ChangeView(offset, null, null, disableAnimation: true);
        Highlight(animate: false);

        bool filtered = _query.Length > 0 || _filter != AllGames;
        RowCaption.Text = _filter != AllGames ? _filter : _query.Length > 0 ? "Search" : "Recently played";
        RowTitle.Text = filtered ? "Your games" : "Jump back in";
        bool none = shown.Count == 0;
        EmptyState.Visibility = none ? Visibility.Visible : Visibility.Collapsed;
        if (none && !ScanRing.IsActive)
            EmptyText.Text = _games.Any(g => !g.Hidden)
                ? "No games match."
                : "No games yet. Use the round buttons at the top to scan for installed games or add one yourself.";
    }

    /// A game's genre text can list more than one ("Action, RPG").
    private static bool HasGenre(LibraryGame g, string genre) =>
        g.Genre.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Any(x => x.Equals(genre, StringComparison.CurrentCultureIgnoreCase));

    /// Show which game is highlighted: its name on the tile, the tile drawn
    /// larger, the row slid so it is in view, and its artwork behind the page.
    private void Highlight(bool animate)
    {
        foreach (object o in GameGrid.Items)
        {
            if (o is not LibraryItem it) continue;
            bool on = ReferenceEquals(it.Game, _selected);
            it.Highlighted = on;
            if (GameGrid.ContainerFromItem(o) is not GridViewItem c) continue;
            SetZoom(c, on, animate);
            if (on && animate && !_fromHover)
                c.StartBringIntoView(new BringIntoViewOptions { AnimationDesired = true, HorizontalAlignmentRatio = 0.35 });
        }

        ShowInfo();

        ShowGameBackdrop(resetSlideshow: true);
    }

    // ================= the name under the highlighted tile =================

    private static ScrollViewer? FindScroller(DependencyObject root)
    {
        for (int i = 0; i < VisualTreeHelper.GetChildrenCount(root); i++)
        {
            var child = VisualTreeHelper.GetChild(root, i);
            if (child is ScrollViewer sv) return sv;
            if (FindScroller(child) is { } found) return found;
        }
        return null;
    }

    /// Fill in the name and the line under it; fade in when the game changes.
    private void ShowInfo()
    {
        var g = _selected;
        bool changed = !ReferenceEquals(g, _infoFor);
        _infoFor = g;
        InfoTitle.Text = g?.Name ?? "";
        ShowSubLine();
        if (changed && g is not null)
        {
            InfoPanel.Opacity = 0;
            Animate(InfoPanel, "Opacity", 1, 220);
        }
        // The tile may only just have been laid out or started sliding: place the text once it has.
        PlaceInfo();
        DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, PlaceInfo);
    }

    /// The second line: a message if one is showing, otherwise the most useful
    /// thing about the game (not installed / time played / genre).
    private void ShowSubLine()
    {
        InfoIcon.Visibility = Visibility.Collapsed;
        if (_status.Length > 0)
        {
            InfoSub.Text = _status;
            return;
        }
        if (_selected is not { } g)
        {
            InfoSub.Text = "";
            return;
        }
        bool onDisk = (g.Path.Length > 0 && File.Exists(g.Path)) || (g.Folder.Length > 0 && Directory.Exists(g.Folder));
        if (!onDisk && g.SteamAppId == 0)
        {
            InfoIcon.Glyph = "\uE896"; // download arrow
            InfoIcon.Visibility = Visibility.Visible;
            InfoSub.Text = g.Exe.Length == 0 && g.Path.Length == 0 ? "PROGRAM NOT SET" : "NOT INSTALLED";
            return;
        }
        if (g.PlayedMinutes > 0 || g.LastPlayed is not null)
        {
            InfoIcon.Glyph = "\uE768"; // play triangle
            InfoIcon.Visibility = Visibility.Visible;
            var parts = new List<string>();
            if (g.PlayedMinutes > 0) parts.Add("PLAYED " + PlayTime.Format(g.PlayedMinutes).ToUpperInvariant());
            if (g.LastPlayed is DateTime when) parts.Add("LAST PLAYED " + WhenText(when));
            InfoSub.Text = string.Join("  \u00B7  ", parts);
            return;
        }
        InfoSub.Text = (g.Genre.Length > 0 ? g.Genre : g.Source == "Manual" ? "Added by you" : g.Source).ToUpperInvariant();
    }

    private static string WhenText(DateTime when)
    {
        int days = (DateTime.Now.Date - when.Date).Days;
        return days <= 0 ? "TODAY" : days == 1 ? "YESTERDAY" : days < 14 ? $"{days} DAYS AGO" : when.ToString("d MMM yyyy").ToUpperInvariant();
    }

    /// Line the text up with the highlighted tile's left edge. It may run on
    /// under the next tiles; it only shortens at the window's edge.
    private void PlaceInfo()
    {
        if (_selected is null || GameGrid.SelectedItem is null || InfoLayer.ActualWidth <= 0) return;
        if (GameGrid.ContainerFromItem(GameGrid.SelectedItem) is not GridViewItem tile || tile.ActualWidth <= 0) return;
        try
        {
            // Measured from the tile's centre, which the zoom doesn't move, so mid-animation is fine.
            var centre = tile.TransformToVisual(InfoLayer).TransformPoint(new Windows.Foundation.Point(tile.ActualWidth / 2, 0));
            double left = Math.Max(0, centre.X - tile.ActualWidth / 2 + 8); // 8 = the tile's own margin
            Canvas.SetLeft(InfoPanel, left);
            InfoPanel.MaxWidth = Math.Max(120, InfoLayer.ActualWidth - left - 8);
        }
        catch (ArgumentException) { /* the tile was just recycled: the next scroll event places it */ }
    }

    /// A message under the highlighted game for a few seconds ("Starting...", "3 new games added").
    private void SetStatus(string message)
    {
        _status = message;
        ShowSubLine();
        _statusTimer ??= CreateStatusTimer();
        _statusTimer.Stop();
        if (message.Length > 0) _statusTimer.Start();
    }

    private Microsoft.UI.Dispatching.DispatcherQueueTimer CreateStatusTimer()
    {
        var t = DispatcherQueue.CreateTimer();
        t.Interval = TimeSpan.FromSeconds(5);
        t.IsRepeating = false;
        t.Tick += (_, _) =>
        {
            _status = "";
            ShowSubLine();
        };
        return t;
    }

    private void Game_Selected(object sender, SelectionChangedEventArgs e)
    {
        if (_loading) return;
        _selected = (GameGrid.SelectedItem as LibraryItem)?.Game;
        Highlight(animate: true);
    }

    /// One click on a tile opens that game's details.
    private void Game_Click(object sender, ItemClickEventArgs e)
    {
        if (e.ClickedItem is not LibraryItem it) return;
        if (!ReferenceEquals(GameGrid.SelectedItem, it)) GameGrid.SelectedItem = it;
        OpenDetails();
    }

    private void Game_RightTapped(object sender, RightTappedRoutedEventArgs e)
    {
        if (e.OriginalSource is FrameworkElement { DataContext: LibraryItem it })
        {
            GameGrid.SelectedItem = it;
            OpenDetails();
        }
    }

    /// Cross / A on a tile opens its details (Cross again on PLAY starts it).
    /// Called by the main window with the focused element; false means
    /// "not mine, carry on as usual".
    public bool ActivateFocused(FrameworkElement focused)
    {
        if (DetailsView.Visibility == Visibility.Visible) return false;
        if (focused is GridViewItem c && GameGrid.ItemFromContainer(c) is LibraryItem it)
        {
            GameGrid.SelectedItem = it;
            OpenDetails();
            return true;
        }
        return false;
    }

    // ================= search and filter =================

    private void Search_TextChanged(object sender, TextChangedEventArgs e)
    {
        _query = SearchBox.Text.Trim();
        ShowGames();
    }

    private void Filter_Click(object sender, RoutedEventArgs e)
    {
        var genres = _games.Where(g => !g.Hidden)
            .SelectMany(g => g.Genre.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
            .Distinct(StringComparer.CurrentCultureIgnoreCase)
            .OrderBy(g => g, StringComparer.CurrentCultureIgnoreCase);
        var menu = new MenuFlyout();
        foreach (string choice in new[] { AllGames, Favourites }.Concat(genres))
        {
            var item = new ToggleMenuFlyoutItem { Text = choice, IsChecked = choice.Equals(_filter, StringComparison.CurrentCultureIgnoreCase) };
            string mine = choice;
            item.Click += (_, _) =>
            {
                _filter = mine;
                ShowGames();
            };
            menu.Items.Add(item);
        }
        menu.ShowAt(FilterButton);
    }

    // ================= details screen =================

    /// Options / Menu on the pad: open the highlighted game's details, or close them.
    public void ToggleDetails()
    {
        if (!CloseDetails()) OpenDetails();
    }

    private void OpenDetails()
    {
        if (_selected is null) return;
        FillDetails();
        LibraryView.IsHitTestVisible = false;
        DetailsView.Visibility = Visibility.Visible;
        Animate(DetailsView, "Opacity", 1, 220);
        // Keyboard focus, so PLAY shows its violet outline straight away, as on a console.
        PlayButton.Focus(FocusState.Keyboard);
        MeasureSizeIfNeeded(_selected);
    }

    /// Returns true if the details were open (Circle / B uses this to go back one step).
    public bool CloseDetails()
    {
        if (DetailsView.Visibility != Visibility.Visible) return false;
        StopTrailer();
        DetailsView.Opacity = 0;
        DetailsView.Visibility = Visibility.Collapsed;
        // Wherever the mouse is resting now (e.g. where a picture was clicked) is not a hover.
        _hoverAt = _lastPointer;
        LibraryView.IsHitTestVisible = true;
        if (GameGrid.SelectedItem is not null && GameGrid.ContainerFromItem(GameGrid.SelectedItem) is Control tile)
            tile.Focus(FocusState.Keyboard);
        return true;
    }

    private void Back_Click(object sender, RoutedEventArgs e) => CloseDetails();

    private void FillDetails()
    {
        if (_selected is not { } g) return;
        StopTrailer();
        var cover = LoadPicture(g.Artwork, 600);
        DetailArt.Source = cover;
        DetailInitials.Text = cover is null ? InitialsOf(g.Name) : "";
        string widePath = g.Background.Length > 0 ? g.Background : g.Artwork;
        DetailWideBrush.ImageSource = LoadPicture(widePath, 1600);
        DetailBlurBrush.ImageSource = LoadPicture(widePath, 40); // tiny, then stretched: a soft blur
        DetailName.Text = g.Name;
        DetailMeta.Text = MetaLine(g);

        FactLastPlayed.Text = g.LastPlayed is DateTime t ? t.ToString("d") : "Not yet";
        FactTimePlayed.Text = PlayTime.Format(g.PlayedMinutes);
        FactReleased.Text = g.ReleaseDate.Length > 0 ? g.ReleaseDate : "Unknown";

        DetailDescription.Text = g.Description.Length > 0
            ? g.Description
            : "No description yet. Choose GAME DETAILS, then Edit, to add one.";
        RawgCredit.Visibility = g.DetailsFrom == "RAWG" ? Visibility.Visible : Visibility.Collapsed; // RAWG asks for a link back
        TrailerButton.Visibility = g.TrailerUrl.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
        FillBackdropPicker(g);

        AchievementsButton.Visibility = g.SteamAppId != 0 ? Visibility.Visible : Visibility.Collapsed;
        string? launcher = LauncherName(g);
        LauncherButton.Visibility = launcher is null ? Visibility.Collapsed : Visibility.Visible;
        ToolTipService.SetToolTip(LauncherButton, launcher is null ? null : $"Open in {launcher}");
        FavouriteItem.Text = g.Favourite ? "Remove from favourites" : "Add to favourites";
        FolderItem.IsEnabled = GameLibrary.InstallFolder(g).Length > 0;
    }

    /// Small pictures of the game's backdrops along the bottom of the wide
    /// picture; picking one makes it the game's backdrop. Only with "pick one"
    /// in Settings and more than one backdrop.
    private void FillBackdropPicker(LibraryGame g)
    {
        BackdropPicker.Children.Clear();
        var pictures = GameLibrary.AllBackdrops(g);
        bool show = App.Settings.BackdropMode != "Slideshow" && pictures.Count > 1;
        BackdropPicker.Visibility = show ? Visibility.Visible : Visibility.Collapsed;
        if (!show) return;
        var accent = (Brush)Application.Current.Resources["DxoAccent"];
        foreach (string path in pictures)
        {
            bool chosen = string.Equals(path, g.Background, StringComparison.OrdinalIgnoreCase);
            var thumb = new Button
            {
                Width = 120,
                Height = 68,
                Padding = new Thickness(0),
                CornerRadius = new CornerRadius(4),
                BorderThickness = new Thickness(chosen ? 3 : 1),
                BorderBrush = chosen ? accent : new SolidColorBrush(Windows.UI.Color.FromArgb(0x66, 0xFF, 0xFF, 0xFF)),
                Content = new Image { Source = LoadPicture(path, 240), Stretch = Stretch.UniformToFill },
                FocusVisualPrimaryBrush = accent,
                Tag = path,
            };
            ToolTipService.SetToolTip(thumb, chosen ? "This game's backdrop" : "Use this backdrop");
            string mine = path;
            thumb.Click += (_, _) =>
            {
                g.Background = mine;
                GameLibrary.Save(_games);
                DetailWideBrush.ImageSource = LoadPicture(mine, 1600);
                DetailBlurBrush.ImageSource = LoadPicture(mine, 40);
                FillBackdropPicker(g);
                // The clicked button was just replaced: put focus on its new copy, so it
                // doesn't fall through to the game row hidden behind the details screen.
                if (BackdropPicker.Children.OfType<Button>().FirstOrDefault(b => ReferenceEquals(b.Tag, mine)) is { } again)
                    again.Focus(FocusState.Programmatic);
                ShowGameBackdrop(resetSlideshow: false); // ready behind the details screen
            };
            BackdropPicker.Children.Add(thumb);
        }
    }

    /// "Developer  ·  2020  ·  71.4 GB", leaving out what isn't known.
    private static string MetaLine(LibraryGame g)
    {
        var parts = new List<string> { g.Developer.Length > 0 ? g.Developer : g.Source == "Manual" ? "Added by you" : g.Source };
        var year = Regex.Match(g.ReleaseDate, @"\b(19|20)\d{2}\b");
        if (year.Success) parts.Add(year.Value);
        if (g.SizeBytes > 0) parts.Add($"{g.SizeBytes / 1e9:0.0} GB");
        return string.Join("  \u2022  ", parts);
    }

    /// Size on disk, measured in the background (big games take a few seconds)
    /// and remembered for a week.
    private async void MeasureSizeIfNeeded(LibraryGame g)
    {
        try
        {
            if (g.SizeBytes > 0 && g.SizeMeasured is DateTime when && DateTime.Now - when < TimeSpan.FromDays(7)) return;
            string folder = GameLibrary.InstallFolder(g);
            if (folder.Length == 0) return;
            long size = await Task.Run(() => GameLibrary.FolderSize(folder));
            if (size <= 0) return;
            g.SizeBytes = size;
            g.SizeMeasured = DateTime.Now;
            GameLibrary.Save(_games);
            if (ReferenceEquals(g, _selected) && DetailsView.Visibility == Visibility.Visible) DetailMeta.Text = MetaLine(g);
        }
        catch (Exception ex) { App.LogCrash("LibraryPage.Size", ex); }
    }

    private static string? LauncherName(LibraryGame g) =>
        g.SteamAppId != 0 || g.Source == "Steam" ? "Steam" : g.Source == "Epic" ? "Epic Games" : null;

    /// Open a steam:// or web address the way a double-click would (as you, not as administrator).
    private void OpenAddress(string address)
    {
        try { Process.Start(new ProcessStartInfo("explorer.exe", $"\"{address}\"") { UseShellExecute = false }); }
        catch (Exception ex)
        {
            App.LogCrash("LibraryPage.Open", ex);
            SetStatus($"Couldn't open {address}: {ex.Message}");
        }
    }

    private void Achievements_Click(object sender, RoutedEventArgs e)
    {
        if (_selected is { SteamAppId: > 0 } g)
            OpenAddress($"steam://openurl/https://steamcommunity.com/stats/{g.SteamAppId}/achievements");
    }

    private void Launcher_Click(object sender, RoutedEventArgs e)
    {
        if (_selected is not { } g) return;
        if (g.SteamAppId != 0) OpenAddress($"steam://nav/games/details/{g.SteamAppId}");
        else if (g.Source == "Steam") OpenAddress("steam://open/games");
        else if (g.Source == "Epic") OpenAddress("com.epicgames.launcher://store");
    }

    private void Folder_Click(object sender, RoutedEventArgs e)
    {
        if (_selected is null) return;
        string folder = GameLibrary.InstallFolder(_selected);
        if (folder.Length > 0) OpenAddress(folder);
    }

    // ---- trailer ----

    private void Trailer_Click(object sender, RoutedEventArgs e)
    {
        if (_selected is not { TrailerUrl.Length: > 0 } g) return;
        try
        {
            // The panel creates and owns its own player. DXO only ever pauses it and
            // clears what it plays: discarding a player the panel still uses crashes the app.
            Trailer.AutoPlay = true;
            Trailer.Source = MediaSource.CreateFromUri(new Uri(g.TrailerUrl));
            var player = Trailer.MediaPlayer;
            if (player is not null && !ReferenceEquals(player, _player))
            {
                _player = player;
                player.MediaFailed += (_, _) => DispatcherQueue.TryEnqueue(() =>
                {
                    StopTrailer();
                    SetStatus("The trailer couldn't be played.");
                    TrailerButton.Visibility = Visibility.Collapsed;
                });
            }
            Trailer.Visibility = Visibility.Visible;
            TrailerButton.Visibility = Visibility.Collapsed;
            DetailWide.Visibility = Visibility.Collapsed;
        }
        catch (Exception ex)
        {
            App.LogCrash("LibraryPage.Trailer", ex);
            StopTrailer();
        }
    }

    private void StopTrailer()
    {
        try
        {
            Trailer.MediaPlayer?.Pause();
            Trailer.Source = null;
        }
        catch (Exception ex) { App.LogCrash("LibraryPage.StopTrailer", ex); }
        Trailer.Visibility = Visibility.Collapsed;
        DetailWide.Visibility = Visibility.Visible;
        TrailerButton.Visibility = _selected is { TrailerUrl.Length: > 0 } ? Visibility.Visible : Visibility.Collapsed;
    }

    // ---- play and the GAME DETAILS menu ----

    private void Play_Click(object sender, RoutedEventArgs e)
    {
        if (_selected is not null) Play(_selected);
    }

    private void Play(LibraryGame g)
    {
        string? exe = GameLibrary.ResolveExe(g);
        if (exe is null)
        {
            // A Steam game DXO can't find on disk can still be started through Steam.
            if (g.SteamAppId != 0)
            {
                OpenAddress($"steam://rungameid/{g.SteamAppId}");
                Started(g);
                return;
            }
            SetStatus($"DXO doesn't know where {g.Name} is installed. Choose GAME DETAILS, then Edit, and pick its .exe file.");
            CloseDetails();
            return;
        }
        try
        {
            // DXO runs as administrator; Explorer starts the game as you, like a double-click would.
            Process.Start(new ProcessStartInfo("explorer.exe", $"\"{exe}\"") { UseShellExecute = false });
            Started(g);
        }
        catch (Exception ex)
        {
            App.LogCrash("LibraryPage.Play", ex);
            SetStatus($"Couldn't start {g.Name}: {ex.Message}");
        }
    }

    private void Started(LibraryGame g)
    {
        g.LastPlayed = DateTime.Now;
        GameLibrary.Save(_games);
        CloseDetails();
        ShowGames(); // it moves to the front of the row
        SetStatus($"Starting {g.Name}...");
    }

    private void Favourite_Click(object sender, RoutedEventArgs e)
    {
        if (_selected is null) return;
        _selected.Favourite = !_selected.Favourite;
        Changed();
    }

    private void Profiles_Click(object sender, RoutedEventArgs e)
    {
        if (((App)Application.Current).MainWindowRef is MainWindow w) w.ShowGameProfiles();
    }

    private async void Remove_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            if (_selected is not { } g) return;
            var dlg = new ContentDialog
            {
                XamlRoot = XamlRoot,
                Title = $"Remove {g.Name} from the library?",
                Content = "The game itself is not uninstalled, and its profiles are kept.",
                PrimaryButtonText = "Remove",
                CloseButtonText = "Cancel",
                DefaultButton = ContentDialogButton.Close,
            };
            if (await dlg.ShowAsync() != ContentDialogResult.Primary) return;
            // A scanned game is hidden, not deleted, so the next scan doesn't bring it back.
            if (g.Source == "Manual") _games.Remove(g);
            else g.Hidden = true;
            _selected = null;
            CloseDetails();
            Changed();
        }
        catch (Exception ex) { App.LogCrash("LibraryPage.Remove", ex); }
    }

    // ================= scan, add, edit =================

    private async void Scan_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            await ScanAsync();
            await FillMissingAsync();
        }
        catch (Exception ex) { App.LogCrash("LibraryPage.Scan", ex); }
    }

    private async Task ScanAsync()
    {
        ScanRing.IsActive = true;
        EmptyText.Text = "Looking for installed games...";
        if (!_games.Any(g => !g.Hidden)) EmptyState.Visibility = Visibility.Visible;
        SetStatus("Looking for installed games...");
        var found = await Task.Run(() => GameScanner.Scan());
        ScanRing.IsActive = false;
        int added = GameLibrary.Merge(_games, found);
        Changed();
        SetStatus(added == 0 ? "No new games found." : added == 1 ? "1 new game added." : $"{added} new games added.");
    }

    private async void Add_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            var g = new LibraryGame();
            if (!await EditAsync(g, isNew: true)) return;
            _games.Add(g);
            _selected = g;
            Changed();
        }
        catch (Exception ex) { App.LogCrash("LibraryPage.Add", ex); }
    }

    private async void Edit_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            if (_selected is { } g && await EditAsync(g, isNew: false)) Changed();
        }
        catch (Exception ex) { App.LogCrash("LibraryPage.Edit", ex); }
    }

    /// Save, refresh the page, and tell the engine which exes are games.
    private void Changed()
    {
        GameLibrary.Save(_games);
        ShowGames();
        if (DetailsView.Visibility == Visibility.Visible)
        {
            if (_selected is null) CloseDetails();
            else FillDetails();
        }
        App.Engine?.SetKnownGames(_games.Where(g => g.Exe.Length > 0).Select(g => g.Exe));
    }

    /// A game's pictures or details changed in the background: update its tile,
    /// and if it is the highlighted one, its name line, backdrop and details.
    /// The row itself is left alone, so it doesn't jump while downloads finish.
    private void RefreshGame(LibraryGame g)
    {
        foreach (object o in GameGrid.Items)
            if (o is LibraryItem it && ReferenceEquals(it.Game, g)) it.Refresh();
        if (!ReferenceEquals(g, _selected)) return;
        ShowInfo();
        ShowGameBackdrop(resetSlideshow: false); // fades only if the picture actually changed
        // Refill the details screen, unless a trailer is playing (refilling stops it).
        if (DetailsView.Visibility == Visibility.Visible && Trailer.Source is null) FillDetails();
    }

    /// Games missing details or pictures: look them up online, a few at a
    /// time, quietly. Each game is asked about once.
    private async Task FillMissingAsync()
    {
        if (_filling || !GameMetadata.Enabled) return;
        _filling = true;
        try
        {
            var todo = _games.Where(g => !g.Hidden && GameMetadata.NeedsLookup(g)).Take(30).ToList();
            foreach (var g in todo)
            {
                if (_leaving.IsCancellationRequested) break;
                bool changed = await GameMetadata.FillMissingAsync(g, _leaving.Token);
                GameLibrary.Save(_games);
                if (changed) RefreshGame(g);
                await Task.Delay(1200, _leaving.Token); // stay well inside the sources' limits
            }
        }
        catch (OperationCanceledException) { /* the page was left */ }
        catch (Exception ex) { App.LogCrash("LibraryPage.Fill", ex); }
        finally { _filling = false; }
    }

    /// The add / edit dialog. Typing a name suggests matching games; picking
    /// one fills the details in and fetches its pictures on Save. Ignoring the
    /// suggestions leaves everything to the player. Returns true if saved.
    private async Task<bool> EditAsync(LibraryGame g, bool isNew)
    {
        var name = new TextBox { Header = "Name", Text = g.Name, PlaceholderText = "Start typing the game's name" };
        var suggestions = new ListView
        {
            SelectionMode = ListViewSelectionMode.None,
            IsItemClickEnabled = true,
            MaxHeight = 170,
            Visibility = Visibility.Collapsed,
        };
        var hint = new TextBlock
        {
            Text = GameMetadata.Enabled ? "Pick a match to fill in its details and pictures, or carry on and enter your own." : "",
            TextWrapping = TextWrapping.Wrap,
            Opacity = 0.7,
            FontSize = 12,
        };
        var genre = new TextBox { Header = "Genre", Text = g.Genre, PlaceholderText = "Racing, RPG, Shooter..." };
        var description = new TextBox
        {
            Header = "Description",
            Text = g.Description,
            AcceptsReturn = true,
            TextWrapping = TextWrapping.Wrap,
            Height = 96,
        };

        // ---- online suggestions ----
        GameDetails? picked = null;
        bool fillingIn = false;
        CancellationTokenSource? typing = null;
        name.TextChanged += async (_, _) =>
        {
            if (fillingIn || !GameMetadata.Enabled) return;
            typing?.Cancel();
            var mine = typing = new CancellationTokenSource();
            try
            {
                await Task.Delay(450, mine.Token); // wait for a pause in the typing
                var found = await GameMetadata.SuggestAsync(name.Text, mine.Token);
                if (mine.IsCancellationRequested) return;
                suggestions.ItemsSource = found;
                suggestions.Visibility = found.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
            }
            catch (OperationCanceledException) { /* another letter was typed */ }
            catch (Exception ex) { App.LogCrash("LibraryPage.Suggest", ex); }
        };
        suggestions.ItemClick += async (_, e) =>
        {
            if (e.ClickedItem is not GameSuggestion s) return;
            try
            {
                typing?.Cancel();
                hint.Text = "Getting the details...";
                var d = await GameMetadata.DetailsAsync(s, _leaving.Token);
                if (d is null)
                {
                    hint.Text = "Couldn't get the details just now. You can still fill them in yourself.";
                    return;
                }
                fillingIn = true;
                name.Text = d.Name;
                if (d.Genre.Length > 0) genre.Text = d.Genre;
                if (d.Description.Length > 0) description.Text = d.Description;
                fillingIn = false;
                picked = d;
                suggestions.Visibility = Visibility.Collapsed;
                hint.Text = $"Filled in from {d.Source}. Its pictures are downloaded when you save.";
            }
            catch (OperationCanceledException) { }
            catch (Exception ex)
            {
                fillingIn = false;
                App.LogCrash("LibraryPage.Details", ex);
            }
        };

        // ---- the game's program ----
        var path = new TextBox
        {
            Header = "Game program (.exe)",
            Text = g.Path.Length > 0 ? g.Path : g.Exe,
            PlaceholderText = @"C:\Games\MyGame\game.exe",
        };
        var browse = new Button { Content = "Browse...", VerticalAlignment = VerticalAlignment.Bottom };
        browse.Click += (_, _) =>
        {
            string? file = FileDialogs.Open("Pick the game's program", ("Programs", "*.exe"), ("All files", "*.*"));
            if (file is null) return;
            path.Text = file;
            if (name.Text.Trim().Length == 0) name.Text = System.IO.Path.GetFileNameWithoutExtension(file);
        };
        var pathRow = new Grid { ColumnSpacing = 8 };
        pathRow.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        pathRow.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        Grid.SetColumn(browse, 1);
        pathRow.Children.Add(path);
        pathRow.Children.Add(browse);

        // ---- pictures chosen by hand (these win over downloaded ones) ----
        string? newCover = null, newWide = null;
        StackPanel PictureRow(string button, string current, Action<string> chosen)
        {
            var label = new TextBlock { Text = current, VerticalAlignment = VerticalAlignment.Center, TextTrimming = TextTrimming.CharacterEllipsis };
            var pick = new Button { Content = button };
            pick.Click += (_, _) =>
            {
                string? file = FileDialogs.Open("Pick a picture", ("Pictures", "*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.webp"), ("All files", "*.*"));
                if (file is null) return;
                chosen(file);
                label.Text = System.IO.Path.GetFileName(file);
            };
            var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
            row.Children.Add(pick);
            row.Children.Add(label);
            return row;
        }

        var form = new StackPanel { Spacing = 10, Width = 460 };
        form.Children.Add(name);
        form.Children.Add(suggestions);
        form.Children.Add(hint);
        form.Children.Add(genre);
        form.Children.Add(description);
        form.Children.Add(pathRow);
        form.Children.Add(PictureRow("Cover picture...", g.Artwork.Length > 0 ? "A cover is set." : "No cover yet (upright, like a game box, fits best).", f => newCover = f));
        // Animated background: a GIF or a short video; Remove clears one already set.
        string? newAnim = null;
        bool clearAnim = false;
        var animLabel = new TextBlock
        {
            Text = g.AnimatedBackground.Length > 0 ? System.IO.Path.GetFileName(g.AnimatedBackground) : "None (optional: GIF, MP4, WebM or MOV)",
            VerticalAlignment = VerticalAlignment.Center,
            TextTrimming = TextTrimming.CharacterEllipsis,
            MaxWidth = 230,
        };
        var animPick = new Button { Content = "Animated background..." };
        animPick.Click += (_, _) =>
        {
            string? file = FileDialogs.Open("Pick a GIF or a short video", ("GIFs and videos", "*.gif;*.mp4;*.webm;*.mov;*.m4v"), ("All files", "*.*"));
            if (file is null) return;
            newAnim = file;
            clearAnim = false;
            animLabel.Text = System.IO.Path.GetFileName(file);
        };
        var animClear = new Button { Content = "Remove" };
        animClear.Click += (_, _) =>
        {
            newAnim = null;
            clearAnim = true;
            animLabel.Text = "None";
        };
        var animRow = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
        animRow.Children.Add(animPick);
        animRow.Children.Add(animClear);
        animRow.Children.Add(animLabel);

        form.Children.Add(PictureRow("Background picture...", g.Background.Length > 0 ? "A background is set." : "No background yet (wide, like a screenshot).", f => newWide = f));
        form.Children.Add(animRow);

        var dlg = new ContentDialog
        {
            XamlRoot = XamlRoot,
            Title = isNew ? "Add a game" : "Edit game",
            Content = new ScrollViewer { Content = form },
            PrimaryButtonText = "Save",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Primary,
        };
        var result = await dlg.ShowAsync();
        typing?.Cancel();
        if (result != ContentDialogResult.Primary) return false;

        string typedPath = path.Text.Trim().Trim('"');
        string typedName = name.Text.Trim();
        if (typedName.Length == 0) typedName = System.IO.Path.GetFileNameWithoutExtension(typedPath);
        if (typedName.Length == 0)
        {
            SetStatus("A game needs a name. Nothing was saved.");
            return false;
        }
        g.Name = typedName;
        g.Genre = genre.Text.Trim();
        g.Description = description.Text.Trim();
        if (typedPath.Length > 0)
        {
            // A full path can be launched; a bare "game.exe" is still enough for profiles.
            bool full = typedPath.Contains('\\') || typedPath.Contains('/');
            g.Path = full ? typedPath : "";
            g.Exe = System.IO.Path.GetFileName(typedPath);
        }

        if (newCover is not null && !GameLibrary.ImportArt(g, newCover))
            SetStatus("That cover couldn't be used (png, jpg, bmp, gif or webp only).");
        if (clearAnim) GameLibrary.ClearAnimated(g);
        if (newAnim is not null && !GameLibrary.ImportAnimated(g, newAnim))
            SetStatus("That animated background couldn't be used (GIF, MP4, WebM or MOV, under 500 MB).");
        if (newWide is not null && !GameLibrary.ImportArt(g, newWide, backdrop: true))
            SetStatus("That background couldn't be used (png, jpg, bmp, gif or webp only).");

        if (picked is not null)
        {
            g.SteamAppId = picked.SteamAppId;
            g.Developer = picked.Developer;
            g.ReleaseDate = picked.ReleaseDate;
            g.TrailerUrl = picked.TrailerUrl;
            g.DetailsFrom = picked.Source;
            SetStatus($"Downloading pictures for {g.Name}...");
            try
            {
                bool got = await GameMetadata.ApplyArtAsync(g, picked, cover: newCover is null, backdrop: newWide is null, _leaving.Token);
                SetStatus(got ? $"{g.Name} saved with its pictures." : $"{g.Name} saved. No pictures could be downloaded.");
            }
            catch (OperationCanceledException) { }
            g.BackdropTried = true;
            g.DetailsTried = true;
        }
        return true;
    }
}
