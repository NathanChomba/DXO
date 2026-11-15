using System;
using System.Collections.Generic;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Net.Http;
using System.Text.Json;
using System.Threading.Tasks;
using DXO.UI.Native;
using DXO.UI.ViewModels;

namespace DXO.UI.Services;

/// Moving profiles in and out of DXO: single files, whole backups, and the
/// opt-in community hub. Everything that comes IN goes through the engine's
/// own parser, and is checked for "open an app" actions first, because a
/// profile from somebody else could otherwise run a program at the press of a
/// button.
public static class ProfileTransfer
{
    // ---------------- safety ----------------

    private static IEnumerable<ActionModel> Actions(ProfileModel m)
    {
        foreach (var b in m.Bindings.Concat(m.ShiftBindings))
        {
            yield return b.Action;
            if (b.Hold is not null) yield return b.Hold;
            if (b.DoubleTap is not null) yield return b.DoubleTap;
        }
        foreach (var c in m.Chords) yield return c.Action;
    }

    /// What this profile would open ("C:\...\app.exe --flag"), one line each.
    public static List<string> LaunchTargets(ProfileModel m) =>
        Actions(m).Where(a => a.Type == "launch")
                  .Select(a => string.IsNullOrWhiteSpace(a.Args) ? a.Target ?? "" : $"{a.Target} {a.Args}")
                  .Where(t => t.Trim().Length > 0).Distinct().ToList();

    /// Turn every "open an app" action into "nothing".
    public static void StripLaunches(ProfileModel m)
    {
        foreach (var a in Actions(m).Where(a => a.Type == "launch"))
        {
            a.Type = "disabled";
            a.Target = null;
            a.Args = null;
        }
    }

    // ---------------- naming ----------------

    public static string FileFor(string name)
    {
        var safe = new string(name.Select(ch => Path.GetInvalidFileNameChars().Contains(ch) ? '_' : ch).ToArray()).Trim();
        return Path.Combine(App.ProfilesDir, (safe.Length == 0 ? "profile" : safe) + ".toml");
    }

    /// "Elden Ring", "Elden Ring 2", ...: a name no saved profile uses yet.
    public static string FreeName(string wanted)
    {
        wanted = string.IsNullOrWhiteSpace(wanted) ? "Imported profile" : wanted.Trim();
        var taken = ProfileStore.All().Select(p => p.Model.Name).ToHashSet(StringComparer.OrdinalIgnoreCase);
        string name = wanted;
        for (int n = 2; taken.Contains(name) || File.Exists(FileFor(name)); n++) name = $"{wanted} {n}";
        return name;
    }

    /// Add a profile that came from outside under a free name. Games another
    /// profile already claims are dropped from it (two profiles for one game
    /// would fight). Returns the name it was saved as, or null on failure.
    public static string? Add(ProfileModel m)
    {
        Directory.CreateDirectory(App.ProfilesDir);
        var claimed = ProfileStore.All().SelectMany(p => p.Model.MatchProcess).ToHashSet(StringComparer.OrdinalIgnoreCase);
        m.MatchProcess = m.MatchProcess.Where(x => !claimed.Contains(x)).ToList();
        m.Name = FreeName(m.Name);
        if (!DxoEngine.ProfileJsonToFile(m.ToJson(), FileFor(m.Name))) return null;
        App.Engine?.ReloadProfiles();
        return m.Name;
    }

    /// Read a profile file from anywhere. Null if it isn't a valid DXO profile.
    public static ProfileModel? Read(string path)
    {
        try { return ProfileModel.FromJson(DxoEngine.ProfileFileToJson(path)); }
        catch (Exception ex) { App.LogCrash("ProfileTransfer.Read", ex); return null; }
    }

    // ---------------- backup ----------------

    private static string DataDir => Path.GetDirectoryName(App.ProfilesDir)!;
    private static readonly string[] BackupExtras = { "controllers.toml", "settings.json" };

    /// Zip every profile plus the controller and app settings. Returns how many profiles went in.
    public static int Backup(string zipPath)
    {
        if (File.Exists(zipPath)) File.Delete(zipPath);
        using var zip = ZipFile.Open(zipPath, ZipArchiveMode.Create);
        int n = 0;
        Directory.CreateDirectory(App.ProfilesDir);
        foreach (var f in Directory.GetFiles(App.ProfilesDir, "*.toml"))
        {
            zip.CreateEntryFromFile(f, "profiles/" + Path.GetFileName(f));
            n++;
        }
        foreach (var extra in BackupExtras)
        {
            string p = Path.Combine(DataDir, extra);
            if (File.Exists(p)) zip.CreateEntryFromFile(p, extra);
        }
        return n;
    }

    public sealed record BackupContents(List<(string File, ProfileModel Model)> Profiles, bool HasControllers, string TempDir);

    /// Unpack a backup into a temporary folder and read what's in it. Nothing
    /// in DXO changes yet. Entry names are reduced to bare file names, so a
    /// crafted zip can't write outside the folder.
    public static BackupContents? OpenBackup(string zipPath)
    {
        try
        {
            string temp = Path.Combine(Path.GetTempPath(), "dxo-restore-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(temp);
            var profiles = new List<(string, ProfileModel)>();
            bool controllers = false;
            using var zip = ZipFile.OpenRead(zipPath);
            foreach (var e in zip.Entries)
            {
                string name = Path.GetFileName(e.FullName);
                if (name.Length == 0 || e.Length > 1_000_000) continue;
                bool inProfiles = e.FullName.Replace('\\', '/').StartsWith("profiles/", StringComparison.OrdinalIgnoreCase);
                if (inProfiles && name.EndsWith(".toml", StringComparison.OrdinalIgnoreCase))
                {
                    string dest = Path.Combine(temp, name);
                    e.ExtractToFile(dest, overwrite: true);
                    if (Read(dest) is ProfileModel m) profiles.Add((dest, m));
                }
                else if (!inProfiles && name.Equals("controllers.toml", StringComparison.OrdinalIgnoreCase))
                {
                    e.ExtractToFile(Path.Combine(temp, "controllers.toml"), overwrite: true);
                    controllers = true;
                }
            }
            return new BackupContents(profiles, controllers, temp);
        }
        catch (Exception ex)
        {
            App.LogCrash("ProfileTransfer.OpenBackup", ex);
            return null;
        }
    }

    /// Put a backup's profiles back (same file name = replaced). Optionally the
    /// controller settings too, which DXO only reads when it starts.
    public static int Restore(BackupContents b, bool stripLaunches, bool controllers)
    {
        int n = 0;
        Directory.CreateDirectory(App.ProfilesDir);
        foreach (var (file, model) in b.Profiles)
        {
            if (stripLaunches) StripLaunches(model);
            if (DxoEngine.ProfileJsonToFile(model.ToJson(), Path.Combine(App.ProfilesDir, Path.GetFileName(file)))) n++;
        }
        if (controllers && b.HasControllers)
            File.Copy(Path.Combine(b.TempDir, "controllers.toml"), Path.Combine(DataDir, "controllers.toml"), overwrite: true);
        App.Engine?.ReloadProfiles();
        try { Directory.Delete(b.TempDir, recursive: true); } catch { /* temp folder: Windows cleans it up */ }
        return n;
    }

    // ---------------- community hub (opt-in) ----------------

    /// Categories a hub entry can use (anything else shows as "Other").
    /// Keep in step with crates/dxo-core/tests/community_hub.rs.
    public static readonly string[] Categories =
    {
        "Shooter", "Racing", "Action & adventure", "Sports", "Fighting", "Platformer",
        "Strategy", "Flight & simulation", "Emulation", "Desktop", "Other",
    };

    /// One entry in a hub's index.json, plus whether it has been fetched this session.
    public sealed class HubProfile : System.ComponentModel.INotifyPropertyChanged
    {
        public string Name { get; init; } = "";
        public string Game { get; init; } = "";
        public string Category { get; init; } = "Other";
        public string Author { get; init; } = "";
        public string Description { get; init; } = "";
        public string Controller { get; init; } = "";
        public List<string> Tags { get; init; } = new();
        public string Url { get; init; } = "";

        public string Title => Game.Length > 0 && !Game.Equals(Name, StringComparison.OrdinalIgnoreCase) ? $"{Name}  ·  {Game}" : Name;
        public string Subtitle => string.Join("  ·  ", new[] { Author.Length > 0 ? $"by {Author}" : "", ControllerText, Description }.Where(x => x.Length > 0));
        public string ControllerText => Controller.Length == 0 || Controller.Equals("any", StringComparison.OrdinalIgnoreCase) ? "Any controller" : Controller;
        public string Meta => string.Join("  ·  ", new[] { Author.Length > 0 ? $"by {Author}" : "", ControllerText }.Where(x => x.Length > 0));
        public string TagText => Tags.Count == 0 ? "" : string.Join("   ", Tags.Select(t => "#" + t));
        public Microsoft.UI.Xaml.Visibility TagVisibility => Tags.Count == 0 ? Microsoft.UI.Xaml.Visibility.Collapsed : Microsoft.UI.Xaml.Visibility.Visible;

        /// Searchable text: name, game, author, controller, category, tags, description.
        public bool Matches(string query) =>
            query.Length == 0 ||
            query.Split(' ', StringSplitOptions.RemoveEmptyEntries).All(word =>
                new[] { Name, Game, Author, Controller, Category, Description }.Concat(Tags)
                    .Any(f => f.Contains(word, StringComparison.OrdinalIgnoreCase)));

        // ---- download state, for the Get button ----
        private string _state = "";   // "" | "busy" | "added"
        public string State
        {
            get => _state;
            set { _state = value; Changed(nameof(ButtonText)); Changed(nameof(CanGet)); }
        }
        /// Set when a saved profile already has this name.
        public bool HaveIt { get; set; }
        public string ButtonText => _state switch { "busy" => "Getting...", "added" => "Added", _ => HaveIt ? "Get again" : "Get" };
        public bool CanGet => _state.Length == 0;

        public event System.ComponentModel.PropertyChangedEventHandler? PropertyChanged;
        private void Changed(string n) => PropertyChanged?.Invoke(this, new System.ComponentModel.PropertyChangedEventArgs(n));
    }

    private const int MaxDownload = 256 * 1024;

    /// The index.json format this DXO understands ("schema": 1).
    private const int HubSchema = 1;

    private static async Task<string> GetText(string url)
    {
        using var http = new HttpClient { Timeout = TimeSpan.FromSeconds(15), MaxResponseContentBufferSize = MaxDownload };
        http.DefaultRequestHeaders.UserAgent.ParseAdd($"DXO/{App.Version}");
        return await http.GetStringAsync(url);
    }

    /// A hub is just a JSON file on the web:
    ///   { "profiles": [ { "name", "game", "category", "author", "description",
    ///                     "controller", "tags": [...], "url" } ] }
    /// `url` is the profile's .toml, absolute or relative to the index. DXO
    /// only ever downloads from it; nothing is uploaded. See docs/community-hub.md.
    public static async Task<(List<HubProfile> Profiles, string? Error)> HubIndex(string indexUrl)
    {
        var list = new List<HubProfile>();
        if (!Uri.TryCreate(indexUrl.Trim(), UriKind.Absolute, out var index) || index.Scheme != Uri.UriSchemeHttps)
            return (list, "The hub address must start with https://");
        try
        {
            using var doc = JsonDocument.Parse(await GetText(index.ToString()));
            if (!doc.RootElement.TryGetProperty("profiles", out var arr) || arr.ValueKind != JsonValueKind.Array)
                return (list, "That address isn't a DXO profile hub (no \"profiles\" list).");
            if (doc.RootElement.TryGetProperty("schema", out var sv) && sv.ValueKind == JsonValueKind.Number && sv.GetInt32() > HubSchema)
                return (list, "This hub uses a newer list format. Update DXO to see its profiles.");
            foreach (var p in arr.EnumerateArray())
            {
                if (p.ValueKind != JsonValueKind.Object) continue;
                string Str(string k) => p.TryGetProperty(k, out var v) && v.ValueKind == JsonValueKind.String ? (v.GetString() ?? "").Trim() : "";
                if (Str("name").Length == 0 || Str("url").Length == 0) continue;
                if (!Uri.TryCreate(index, Str("url"), out var file) || file.Scheme != Uri.UriSchemeHttps) continue;
                string category = Categories.FirstOrDefault(c => c.Equals(Str("category"), StringComparison.OrdinalIgnoreCase)) ?? "Other";
                var tags = new List<string>();
                if (p.TryGetProperty("tags", out var t) && t.ValueKind == JsonValueKind.Array)
                    foreach (var tag in t.EnumerateArray())
                        if (tag.ValueKind == JsonValueKind.String && tag.GetString() is { Length: > 0 and <= 40 } s) tags.Add(s.Trim());
                list.Add(new HubProfile
                {
                    Name = Str("name"), Game = Str("game"), Category = category, Author = Str("author"),
                    Description = Str("description"), Controller = Str("controller"), Tags = tags.Take(8).ToList(),
                    Url = file.ToString(),
                });
            }
            return (list, list.Count == 0 ? "The hub has no profiles yet." : null);
        }
        catch (Exception ex)
        {
            return (list, $"Couldn't reach the hub: {ex.Message}");
        }
    }

    /// What happened to a hub download.
    public sealed record HubResult(string? Name, int Removed, List<string> Taken, string? Error);

    /// Download one hub profile and add it. "Open an app" actions are always
    /// removed from hub profiles. Games another profile already uses are left
    /// off the new one and listed in `Taken` (see ClaimGames).
    public static async Task<HubResult> HubDownload(HubProfile p)
    {
        string temp = Path.Combine(Path.GetTempPath(), "dxo-hub-" + Guid.NewGuid().ToString("N") + ".toml");
        try
        {
            await File.WriteAllTextAsync(temp, await GetText(p.Url));
            if (Read(temp) is not ProfileModel m) return new(null, 0, new(), "That download isn't a valid DXO profile.");
            int removed = LaunchTargets(m).Count;
            StripLaunches(m);
            if (m.Name.Trim().Length == 0 || m.Name == "Default") m.Name = p.Name;
            var wanted = m.MatchProcess.ToList();
            string? saved = Add(m);
            if (saved is null) return new(null, 0, new(), "The engine rejected that profile.");
            var taken = wanted.Where(x => !m.MatchProcess.Contains(x, StringComparer.OrdinalIgnoreCase)).ToList();
            return new(saved, removed, taken, null);
        }
        catch (Exception ex)
        {
            return new(null, 0, new(), $"Download failed: {ex.Message}");
        }
        finally
        {
            try { File.Delete(temp); } catch { }
        }
    }

    /// Which saved profiles currently use these game .exes (profile names).
    public static List<string> Owners(IEnumerable<string> exes)
    {
        var set = exes.ToHashSet(StringComparer.OrdinalIgnoreCase);
        return ProfileStore.All().Where(p => p.Model.MatchProcess.Any(set.Contains)).Select(p => p.Model.Name).Distinct().ToList();
    }

    /// Move these game .exes to `profileName`: other profiles stop switching
    /// on for them, this one starts. False if a file couldn't be written.
    public static bool ClaimGames(string profileName, IEnumerable<string> exes)
    {
        var set = exes.ToHashSet(StringComparer.OrdinalIgnoreCase);
        bool ok = true;
        foreach (var (path, m) in ProfileStore.All())
        {
            if (m.Name == profileName)
            {
                m.MatchProcess = m.MatchProcess.Concat(set).Distinct(StringComparer.OrdinalIgnoreCase).ToList();
                ok &= DxoEngine.ProfileJsonToFile(m.ToJson(), path);
            }
            else if (m.MatchProcess.Any(set.Contains))
            {
                m.MatchProcess = m.MatchProcess.Where(x => !set.Contains(x)).ToList();
                ok &= DxoEngine.ProfileJsonToFile(m.ToJson(), path);
            }
        }
        App.Engine?.ReloadProfiles();
        return ok;
    }

    /// Profiles fetched this session, so the Community page doesn't re-download
    /// the index every time it opens. Cleared by "Refresh".
    public static List<HubProfile>? HubCache { get; set; }
    public static string HubCacheUrl { get; set; } = "";
}
