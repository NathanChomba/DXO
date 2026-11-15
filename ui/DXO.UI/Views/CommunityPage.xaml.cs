using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using DXO.UI.Services;
using HubProfile = DXO.UI.Services.ProfileTransfer.HubProfile;

namespace DXO.UI.Views;

/// One game on the Community page, with every shared setup for it.
public sealed class CommunityGame
{
    public string Game { get; init; } = "";
    public string Category { get; init; } = "";
    public bool Owned { get; init; }
    public Visibility OwnedVisibility => Owned ? Visibility.Visible : Visibility.Collapsed;
    public List<HubProfile> Profiles { get; init; } = new();
    public string CountText => Profiles.Count == 1 ? "1 setup" : $"{Profiles.Count} setups";
}

/// Browse a community hub by game: search, filter by kind of game and
/// controller, and "Get" a setup. Several setups per game are normal; when
/// one you get is for a game another profile already handles, DXO asks which
/// should switch on for it.
public sealed partial class CommunityPage : Page
{
    private const string AllCategories = "All kinds of game";
    private const string AnyController = "Any controller";
    private const string OtherGames = "Other";

    /// Normalised names of games found on this PC (Steam, Epic, GOG). Scanned once per run.
    private static HashSet<string>? _owned;
    private bool _ready;

    public CommunityPage()
    {
        InitializeComponent();
        CategoryBox.ItemsSource = new[] { AllCategories }.Concat(ProfileTransfer.Categories).ToList();
        CategoryBox.SelectedIndex = 0;
        ControllerBox.ItemsSource = new List<string> { AnyController };
        ControllerBox.SelectedIndex = 0;
    }

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        try
        {
            bool hub = App.Settings.CommunityHub.Length > 0;
            SetupCard.Visibility = hub ? Visibility.Collapsed : Visibility.Visible;
            Toolbar.Visibility = hub ? Visibility.Visible : Visibility.Collapsed;
            if (!hub) { ShowEmpty("No hub connected yet.", busy: false); GroupList.ItemsSource = null; return; }
            await Load(force: false);
        }
        catch (Exception ex) { App.LogCrash("Community.Open", ex); Say(ex.Message, InfoBarSeverity.Error); }
    }

    // ---------------- loading ----------------

    private async Task Load(bool force)
    {
        _ready = false;
        string url = App.Settings.CommunityHub;
        if (force || ProfileTransfer.HubCache is null || ProfileTransfer.HubCacheUrl != url)
        {
            GroupList.ItemsSource = null;
            ShowEmpty("Loading profiles...", busy: true);
            var (profiles, error) = await ProfileTransfer.HubIndex(url);
            if (error is not null && profiles.Count == 0)
            {
                ProfileTransfer.HubCache = null;
                ShowEmpty(error, busy: false);
                return;
            }
            ProfileTransfer.HubCache = profiles;
            ProfileTransfer.HubCacheUrl = url;
        }

        // Which of these do we already have (same name)?
        var have = ProfileStore.All().Select(p => p.Model.Name).ToHashSet(StringComparer.OrdinalIgnoreCase);
        foreach (var p in ProfileTransfer.HubCache) p.HaveIt = have.Contains(p.Name);

        // Controller filter: whatever the hub lists.
        string current = ControllerBox.SelectedItem as string ?? AnyController;
        var pads = ProfileTransfer.HubCache.Select(p => p.ControllerText).Where(c => c != AnyController)
                                           .Distinct(StringComparer.OrdinalIgnoreCase).OrderBy(c => c).ToList();
        pads.Insert(0, AnyController);
        ControllerBox.ItemsSource = pads;
        ControllerBox.SelectedIndex = Math.Max(0, pads.IndexOf(current));

        _owned ??= await Task.Run(() =>
        {
            try { return GameScanner.Scan().Select(g => Key(g.Name)).Where(k => k.Length > 0).ToHashSet(); }
            catch (Exception ex) { App.LogCrash("Community.Scan", ex); return new HashSet<string>(); }
        });

        _ready = true;
        Apply();
    }

    /// "Call of Duty®: Modern Warfare III" -> "callofdutymodernwarfareiii"
    private static string Key(string name) => new(name.Where(char.IsLetterOrDigit).Select(char.ToLowerInvariant).ToArray());

    private static bool IsOwned(string game)
    {
        if (_owned is null || game.Length == 0) return false;
        string k = Key(game);
        return k.Length >= 3 && _owned.Any(o => o.Contains(k) || k.Contains(o) && o.Length >= 3);
    }

    // ---------------- filtering ----------------

    private void Apply()
    {
        if (!_ready || ProfileTransfer.HubCache is not { } all) return;
        string query = SearchBox.Text.Trim();
        string category = CategoryBox.SelectedItem as string ?? AllCategories;
        string pad = ControllerBox.SelectedItem as string ?? AnyController;
        bool mine = MineBox.IsChecked == true;

        var groups = all
            .Where(p => p.Matches(query))
            .Where(p => category == AllCategories || p.Category == category)
            .Where(p => pad == AnyController || p.ControllerText == AnyController || p.ControllerText.Equals(pad, StringComparison.OrdinalIgnoreCase))
            .GroupBy(p => p.Game.Length > 0 ? p.Game : OtherGames, StringComparer.OrdinalIgnoreCase)
            .Select(g => new CommunityGame
            {
                Game = g.First().Game.Length > 0 ? g.First().Game : OtherGames,
                Category = g.GroupBy(p => p.Category).OrderByDescending(c => c.Count()).First().Key,
                Owned = IsOwned(g.Key),
                Profiles = g.OrderBy(p => p.Name, StringComparer.CurrentCultureIgnoreCase).ToList(),
            })
            .Where(g => !mine || g.Owned)
            .OrderByDescending(g => g.Owned)
            .ThenBy(g => g.Game == OtherGames)
            .ThenBy(g => g.Game, StringComparer.CurrentCultureIgnoreCase)
            .ToList();

        GroupList.ItemsSource = groups;
        if (groups.Count > 0) { Empty.Visibility = Visibility.Collapsed; Busy.IsActive = false; }
        else ShowEmpty(all.Count == 0 ? "The hub has no profiles yet."
                       : mine ? "None of the shared profiles are for games found on this PC." : "Nothing matches. Try fewer words or another filter.", busy: false);
    }

    private void Search_TextChanged(AutoSuggestBox sender, AutoSuggestBoxTextChangedEventArgs args) => Apply();
    private void Filter_Changed(object sender, SelectionChangedEventArgs e) => Apply();
    private void Filter_Click(object sender, RoutedEventArgs e) => Apply();

    private async void Refresh_Click(object sender, RoutedEventArgs e)
    {
        try { await Load(force: true); }
        catch (Exception ex) { App.LogCrash("Community.Refresh", ex); Say(ex.Message, InfoBarSeverity.Error); }
    }

    // ---------------- getting a profile ----------------

    private async void Get_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is not HubProfile p || !p.CanGet) return;
        try
        {
            p.State = "busy";
            var r = await ProfileTransfer.HubDownload(p);
            if (r.Name is null)
            {
                p.State = "";
                Say(r.Error ?? "Download failed.", InfoBarSeverity.Error);
                return;
            }
            p.State = "added";
            p.HaveIt = true;

            string msg = $"Added \"{r.Name}\". It's in your Library on the Profiles page.";
            if (r.Removed > 0) msg += $" {r.Removed} \"open a program\" action{(r.Removed == 1 ? " was" : "s were")} removed.";

            // Another profile already switches on for this game: ask which one should.
            if (r.Taken.Count > 0)
            {
                var owners = ProfileTransfer.Owners(r.Taken);
                string game = p.Game.Length > 0 ? p.Game : string.Join(", ", r.Taken);
                var ask = new ContentDialog
                {
                    XamlRoot = XamlRoot,
                    Title = $"Use it for {game}?",
                    Content = new TextBlock
                    {
                        TextWrapping = TextWrapping.Wrap,
                        Text = $"\"{string.Join("\", \"", owners)}\" already switches on when {game} starts. " +
                               $"Switch to \"{r.Name}\" instead? You can change this any time on the Games page.",
                    },
                    PrimaryButtonText = "Use the new one",
                    CloseButtonText = "Keep the current one",
                    DefaultButton = ContentDialogButton.Primary,
                };
                if (await ask.ShowAsync() == ContentDialogResult.Primary)
                    msg += ProfileTransfer.ClaimGames(r.Name, r.Taken)
                        ? $" It now switches on for {game}."
                        : " Couldn't update the game assignment; set it on the Games page.";
                else
                    msg += $" \"{owners.FirstOrDefault()}\" stays the profile for {game}.";
            }
            Say(msg, InfoBarSeverity.Success, withProfilesButton: true);
        }
        catch (Exception ex)
        {
            p.State = "";
            App.LogCrash("Community.Get", ex);
            Say($"Download failed: {ex.Message}", InfoBarSeverity.Error);
        }
    }

    // ---------------- hub address ----------------

    private async void Connect_Click(object sender, RoutedEventArgs e) => await SetHub(SetupBox.Text);

    private void HubFlyout_Opening(object sender, object e) => HubBox.Text = App.Settings.CommunityHub;

    private async void SaveHub_Click(object sender, RoutedEventArgs e)
    {
        HubFlyout.Hide();
        await SetHub(HubBox.Text);
    }

    private void Disconnect_Click(object sender, RoutedEventArgs e)
    {
        HubFlyout.Hide();
        App.Settings.CommunityHub = "";
        App.Settings.Save();
        ProfileTransfer.HubCache = null;
        SetupBox.Text = "";
        SetupCard.Visibility = Visibility.Visible;
        Toolbar.Visibility = Visibility.Collapsed;
        GroupList.ItemsSource = null;
        ShowEmpty("No hub connected yet.", busy: false);
    }

    private async Task SetHub(string address)
    {
        try
        {
            // Pasted from chat or docs: drop quotes, backticks, brackets and trailing punctuation.
            string url = address.Trim().Trim('`', '"', '\'', '<', '>', '(', ')', ',', ';', ' ');
            if (!url.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
            {
                Say("The hub address must start with https:// (the raw address of its index.json).", InfoBarSeverity.Warning);
                return;
            }
            App.Settings.CommunityHub = url;
            App.Settings.Save();
            SetupCard.Visibility = Visibility.Collapsed;
            Toolbar.Visibility = Visibility.Visible;
            Result.IsOpen = false;
            await Load(force: true);
        }
        catch (Exception ex) { App.LogCrash("Community.SetHub", ex); Say(ex.Message, InfoBarSeverity.Error); }
    }

    // ---------------- messages ----------------

    private void ShowEmpty(string text, bool busy)
    {
        EmptyText.Text = text;
        Busy.IsActive = busy;
        Busy.Visibility = busy ? Visibility.Visible : Visibility.Collapsed;
        Empty.Visibility = Visibility.Visible;
    }

    private void Say(string text, InfoBarSeverity severity, bool withProfilesButton = false)
    {
        Result.Message = text;
        Result.Severity = severity;
        if (withProfilesButton)
        {
            var go = new Button { Content = "Open Profiles" };
            go.Click += (_, _) => MainWindow.NavigateTo("profiles");
            Result.ActionButton = go;
        }
        else Result.ActionButton = null;
        Result.IsOpen = true;
    }
}
