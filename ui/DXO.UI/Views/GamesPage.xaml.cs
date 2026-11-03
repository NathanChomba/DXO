using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using DXO.UI.Services;
using DXO.UI.Native;
using DXO.UI.ViewModels;

namespace DXO.UI.Views;

/// One game in the list.
public sealed class GameRow : System.ComponentModel.INotifyPropertyChanged
{
    public string Name { get; init; } = "";
    public string Source { get; init; } = "";
    public string Exe { get; init; } = "";
    public string Subtitle
    {
        get
        {
            int own = Overrides.Count(o => o.Assigned > 0);
            return $"{Source}  ·  {Exe}" + (own == 0 ? "" : own == 1 ? "  ·  1 controller has its own profile" : $"  ·  {own} controllers have their own profile");
        }
    }
    /// "No profile" + every profile name (shared list)
    public List<string> Choices { get; init; } = new();
    /// Index into Choices: 0 = none
    public int Assigned { get; set; }

    /// Per-controller choices for this game (connected controllers only).
    public List<PadOverride> Overrides { get; init; } = new();
    private bool _expanded;
    public bool Expanded
    {
        get => _expanded;
        set { _expanded = value; Changed(nameof(OverridesVisibility)); Changed(nameof(EachGlyph)); }
    }
    public Visibility OverridesVisibility => _expanded ? Visibility.Visible : Visibility.Collapsed;
    public Visibility EachVisibility => Overrides.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
    public string EachLabel => "Each controller";
    public string EachGlyph => _expanded ? "\uE70E" : "\uE70D"; // chevron up / down

    public void Refresh() => Changed(nameof(Subtitle));
    public event System.ComponentModel.PropertyChangedEventHandler? PropertyChanged;
    private void Changed(string n) => PropertyChanged?.Invoke(this, new System.ComponentModel.PropertyChangedEventArgs(n));
}

/// One controller's choice for one game.
public sealed class PadOverride
{
    public int Slot { get; init; }
    public string PadName { get; init; } = "";
    public string PlayerText { get; init; } = "";
    public string Exe { get; init; } = "";
    /// "Same as all controllers" + every profile name
    public List<string> Choices { get; init; } = new();
    /// 0 = same as all controllers
    public int Assigned { get; set; }
    public GameRow? Row { get; set; }
}

/// Detected games + per-game profile assignment. Assigning writes the game's
/// .exe into that profile's `match_process` (and removes it from the others).
public sealed partial class GamesPage : Page
{
    private const string NoProfile = "No profile (use the active one)";
    private const string SameAsAll = "Same as all controllers";

    private static List<FoundGame>? _scanCache; // scanning walks folders; do it once per run
    private readonly List<GameRow> _all = new();
    private readonly ObservableCollection<GameRow> _shown = new();
    private List<(string Path, ProfileModel Model)> _profiles = new();

    public GamesPage()
    {
        InitializeComponent();
        GameList.ItemsSource = _shown;
    }

    protected override async void OnNavigatedTo(NavigationEventArgs e) => await Reload(rescan: false);

    private async Task Reload(bool rescan)
    {
        _profiles = ProfileStore.All();
        if (rescan || _scanCache is null)
        {
            ShowEmpty("Looking for Steam and Epic games...", busy: true);
            try { _scanCache = await Task.Run(() => GameScanner.Scan()); }
            catch (Exception ex) { App.LogCrash("Games scan", ex); _scanCache = new(); }
        }

        var choices = new List<string> { NoProfile };
        choices.AddRange(_profiles.Select(p => p.Model.Name));
        var padChoices = new List<string> { SameAsAll };
        padChoices.AddRange(_profiles.Select(p => p.Model.Name));

        // Connected controllers and their saved per-game rules.
        var pads = new List<(PadInfo Info, ControllerModel Model)>();
        if (App.Engine is { } eng)
            foreach (var p in eng.ListPads())
                if (ControllerModel.FromJson(eng.ControllerJson(p.Slot)) is ControllerModel m) pads.Add((p, m));

        // Scanned games, plus anything profiles already list that the scan didn't find.
        var games = _scanCache!.Select(g => (g.Name, g.Source, g.Exe)).ToList();
        foreach (var exe in _profiles.SelectMany(p => p.Model.MatchProcess))
            if (!games.Any(g => g.Exe.Equals(exe, StringComparison.OrdinalIgnoreCase)))
                games.Add((Path.GetFileNameWithoutExtension(exe), "Added", exe));

        _all.Clear();
        foreach (var g in games.OrderBy(g => g.Name, StringComparer.CurrentCultureIgnoreCase))
        {
            var row = new GameRow
            {
                Name = g.Name, Source = g.Source, Exe = g.Exe, Choices = choices, Assigned = AssignedIndex(g.Exe),
                Overrides = pads.Select(p => new PadOverride
                {
                    Slot = p.Info.Slot, PadName = p.Info.Name, PlayerText = $"Player {p.Info.Slot + 1}", Exe = g.Exe,
                    Choices = padChoices, Assigned = Math.Max(0, padChoices.IndexOf(p.Model.ProfileFor(g.Exe) ?? SameAsAll)),
                }).ToList(),
            };
            foreach (var o in row.Overrides) o.Row = row;
            _all.Add(row);
        }
        Filter();
        int linked = _all.Count(r => r.Assigned > 0);
        StatusText.Text = $"{_all.Count} games  ·  {linked} with their own profile";
    }

    private int AssignedIndex(string exe)
    {
        for (int i = 0; i < _profiles.Count; i++)
            if (_profiles[i].Model.MatchProcess.Any(m => m.Equals(exe, StringComparison.OrdinalIgnoreCase)))
                return i + 1;
        return 0;
    }

    private void Filter()
    {
        string q = SearchBox.Text?.Trim() ?? "";
        _shown.Clear();
        foreach (var r in _all.Where(r => q.Length == 0 || r.Name.Contains(q, StringComparison.CurrentCultureIgnoreCase) ||
                                          r.Exe.Contains(q, StringComparison.OrdinalIgnoreCase)))
            _shown.Add(r);
        if (_all.Count == 0) ShowEmpty("No games found. Use \"Add a game\" to add one by hand.", busy: false);
        else if (_shown.Count == 0) ShowEmpty("Nothing matches your search.", busy: false);
        else EmptyState.Visibility = Visibility.Collapsed;
    }

    private void ShowEmpty(string text, bool busy)
    {
        EmptyState.Visibility = Visibility.Visible;
        EmptyText.Text = text;
        ScanRing.IsActive = busy;
        ScanRing.Visibility = busy ? Visibility.Visible : Visibility.Collapsed;
    }

    private void Search_TextChanged(AutoSuggestBox sender, AutoSuggestBoxTextChangedEventArgs args) => Filter();
    private async void Rescan_Click(object sender, RoutedEventArgs e) => await Reload(rescan: true);

    // ================= assignment =================

    private void Assign_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (sender is not ComboBox cb || cb.DataContext is not GameRow row) return;
        int pick = cb.SelectedIndex;
        if (pick < 0 || pick == row.Assigned) return; // initial binding / recycled row
        if (Assign(row.Exe, pick))
        {
            row.Assigned = pick;
            StatusText.Text = pick == 0 ? $"{row.Name} no longer has its own profile."
                                        : $"{row.Name} will use \"{_profiles[pick - 1].Model.Name}\".";
        }
        else
        {
            cb.SelectedIndex = row.Assigned;
        }
    }

    /// Put `exe` in profile `pick` (1-based; 0 = none) and take it out of every other profile.
    private bool Assign(string exe, int pick)
    {
        var active = ProfileStore.Active();
        bool ok = true;
        for (int i = 0; i < _profiles.Count; i++)
        {
            var (path, m) = _profiles[i];
            bool had = m.MatchProcess.Any(x => x.Equals(exe, StringComparison.OrdinalIgnoreCase));
            bool want = i + 1 == pick;
            if (had == want) continue;
            m.MatchProcess.RemoveAll(x => x.Equals(exe, StringComparison.OrdinalIgnoreCase));
            if (want) m.MatchProcess.Add(exe);
            if (!ProfileStore.Save(m, path)) { ok = false; StatusText.Text = $"Couldn't save \"{m.Name}\"."; continue; }

            // Keep the engine's copy of the active profile in step, or a later
            // "Save to profile" from another page would write the old game list back.
            if (active is not null && active.Name == m.Name)
            {
                active.MatchProcess = m.MatchProcess.ToList();
                ProfileStore.ApplyActive(active);
            }
        }
        return ok;
    }

    // ================= per controller =================

    private void EachController_Click(object sender, RoutedEventArgs e)
    {
        if ((sender as FrameworkElement)?.DataContext is GameRow row) row.Expanded = !row.Expanded;
    }

    /// "In this game, this controller uses ..." -> saved in that controller's settings.
    private void Override_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (sender is not ComboBox cb || cb.DataContext is not PadOverride o || App.Engine is not { } eng) return;
        int pick = cb.SelectedIndex;
        if (pick < 0 || pick == o.Assigned) return; // initial binding / recycled row
        var c = ControllerModel.FromJson(eng.ControllerJson(o.Slot));
        if (c is null) { cb.SelectedIndex = o.Assigned; return; }
        c.SetProfileFor(o.Exe, pick == 0 ? null : o.Choices[pick]);
        if (!eng.SetControllerJson(o.Slot, c.ToJson())) { cb.SelectedIndex = o.Assigned; return; }
        o.Assigned = pick;
        o.Row?.Refresh();
        StatusText.Text = pick == 0
            ? $"{o.PadName} uses the same profile as everyone in {o.Row?.Name}."
            : $"{o.PadName} will use \"{o.Choices[pick]}\" in {o.Row?.Name}.";
    }

    // ================= add by hand =================

    private void AddFlyout_Opening(object sender, object e)
    {
        // Windowed apps running right now, minus Windows itself and DXO.
        var mine = Process.GetCurrentProcess().Id;
        var list = new List<string>();
        foreach (var p in Process.GetProcesses())
        {
            try
            {
                if (p.Id == mine || p.MainWindowHandle == IntPtr.Zero || string.IsNullOrWhiteSpace(p.MainWindowTitle)) continue;
                var exe = p.ProcessName + ".exe";
                if (exe.Equals("explorer.exe", StringComparison.OrdinalIgnoreCase) ||
                    exe.Equals("ApplicationFrameHost.exe", StringComparison.OrdinalIgnoreCase) ||
                    exe.Equals("TextInputHost.exe", StringComparison.OrdinalIgnoreCase)) continue;
                list.Add($"{p.MainWindowTitle}  ({exe})");
            }
            catch { /* access denied for some system processes */ }
            finally { p.Dispose(); }
        }
        RunningBox.ItemsSource = list.Distinct().OrderBy(x => x).ToList();
        ExeBox.Text = "";
    }

    private async void Add_Click(object sender, RoutedEventArgs e)
    {
        string? exe = null, name = null;
        if (RunningBox.SelectedItem is string pick)
        {
            int open = pick.LastIndexOf("  (", StringComparison.Ordinal);
            if (open < 0) return;
            name = pick[..open];
            exe = pick[(open + 3)..].TrimEnd(')');
        }
        else if (!string.IsNullOrWhiteSpace(ExeBox.Text))
        {
            exe = Path.GetFileName(ExeBox.Text.Trim().Trim('"'));
            if (!exe.EndsWith(".exe", StringComparison.OrdinalIgnoreCase)) exe += ".exe";
            name = Path.GetFileNameWithoutExtension(exe);
        }
        if (exe is null) return;
        AddFlyout.Hide();

        _scanCache ??= new();
        if (!_scanCache.Any(g => g.Exe.Equals(exe, StringComparison.OrdinalIgnoreCase)))
            _scanCache.Add(new FoundGame(name ?? exe, "Added", exe, ""));
        await Reload(rescan: false);
        SearchBox.Text = name ?? exe;
        Filter();
        StatusText.Text = $"Added {exe}. Pick a profile for it on the right.";
    }
}
