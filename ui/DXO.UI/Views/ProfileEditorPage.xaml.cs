using System;
using System.Linq;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using Microsoft.UI.Xaml.Navigation;
using DXO.UI.Services;
using DXO.UI.ViewModels;

namespace DXO.UI.Views;

/// Profiles, Buttons, Mouse, Motion, Rumble, Triggers and Lighting in one
/// page with tabs, so it's always clear which profile is being changed.
public sealed partial class ProfileEditorPage : Page
{
    /// Tab shown next time (remembered while the app runs; set by NavigateTo("mappings") etc.).
    public static string LastTab = "library";

    private bool _loading;

    public ProfileEditorPage()
    {
        _loading = true;
        InitializeComponent();
        _loading = false;
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        if (e.Parameter is string tab) LastTab = tab;
        ShowTab(LastTab);
    }

    /// Leaving the Profiles page: the tab's page must get its OnNavigatedFrom too,
    /// or its timers and device events keep running in the background.
    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        Inner.Navigate(typeof(Page), null, new SuppressNavigationTransitionInfo());
        Inner.BackStack.Clear();
    }

    /// Select a tab by tag (library, buttons, mouse, motion, rumble, triggers, lighting).
    public void ShowTab(string tag)
    {
        var item = Tabs.Items.FirstOrDefault(i => (string)i.Tag == tag) ?? Tabs.Items[0];
        if (Tabs.SelectedItem == item) { Open(tag); return; }
        Tabs.SelectedItem = item; // fires Tabs_SelectionChanged -> Open
    }

    /// The selected controller changed: show the new one's profile.
    public void ReloadInner()
    {
        if (LastTab != "library") Open(LastTab);
        else UpdateHeader();
    }

    private void Tabs_SelectionChanged(SelectorBar sender, SelectorBarSelectionChangedEventArgs args)
    {
        if (_loading || sender.SelectedItem?.Tag is not string tag) return;
        LastTab = tag;
        Open(tag);
    }

    private void Open(string tag)
    {
        Type page = tag switch
        {
            "buttons" => typeof(MappingsPage),
            "mouse" => typeof(MousePage),
            "motion" => typeof(MotionPage),
            "rumble" => typeof(HapticsPage),
            "triggers" => typeof(TriggersPage),
            "lighting" => typeof(LightingPage),
            _ => typeof(ProfilesPage),
        };
        Inner.Navigate(page, "embedded", new SuppressNavigationTransitionInfo());
        Inner.BackStack.Clear();
        if (Inner.Content is Page p) Embed(p);
        UpdateHeader();
    }

    /// The tab already says what this is: hide the page's own big title and
    /// tighten its top padding so it sits under the tabs.
    private static void Embed(Page p)
    {
        var content = p.Content is ScrollViewer sv ? sv.Content : p.Content;
        if (content is not Panel root) return;
        if (root is Grid g) g.Padding = new Thickness(g.Padding.Left, 8, g.Padding.Right, g.Padding.Bottom);
        else if (root is StackPanel sp) sp.Padding = new Thickness(sp.Padding.Left, 8, sp.Padding.Right, sp.Padding.Bottom);
        var title = (Style)Application.Current.Resources["DxoPageTitle"];
        HideTitle(root, title);
    }

    private static bool HideTitle(Panel panel, Style title)
    {
        foreach (var child in panel.Children)
        {
            if (child is TextBlock tb && tb.Style == title) { tb.Visibility = Visibility.Collapsed; return true; }
            if (child is Panel inner && HideTitle(inner, title)) return true;
        }
        return false;
    }

    private void UpdateHeader()
    {
        var pad = App.Device.Pads.FirstOrDefault(p => p.IsSelected) ?? App.Device.Pads.FirstOrDefault();
        string profile = ProfileStore.Active()?.Name ?? "";
        if (pad is null)
        {
            EditingText.Text = LastTab == "library"
                ? "All your saved profiles. Connect a controller to edit the one it's using live."
                : "No controller connected: plug one in to edit its profile live.";
            EditingDot.Visibility = Visibility.Collapsed;
            return;
        }
        EditingDot.Visibility = Visibility.Visible;
        int slot = Math.Clamp(pad.Slot, 0, ControllerModel.PlayerColours.Length - 1);
        EditingDot.Background = new SolidColorBrush(ControllerModel.PlayerColours[slot]);
        EditingText.Text = LastTab == "library"
            ? $"All your saved profiles. {pad.PlayerText} ({pad.Name}) is using \"{profile}\"."
            : $"Editing \"{profile}\", the profile {pad.PlayerText} ({pad.Name}) is using right now. Pick another controller in the bar above to edit its profile.";
    }
}
