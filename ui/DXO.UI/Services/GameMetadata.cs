using System;
using System.Collections.Generic;
using System.Linq;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;

namespace DXO.UI.Services;

/// One line in the "is this the game you mean?" list while adding a game.
public sealed record GameSuggestion(string Source, string Id, string Name, string Note)
{
    /// What the list shows under the name: "Steam", or "RAWG · 2020".
    public string Subtitle => Note.Length > 0 ? $"{Source}  ·  {Note}" : Source;

    /// The suggestions list shows this text.
    public override string ToString() => $"{Name}   ({Subtitle})";
}

/// What a source knows about a game. Empty strings where it doesn't say.
public sealed class GameDetails
{
    public string Source = "";
    public string Name = "";
    public string Description = "";
    public string Genre = "";
    public string Developer = "";
    public string ReleaseDate = "";
    public int SteamAppId;
    /// A trailer video, or "".
    public string TrailerUrl = "";
    /// Pictures to try, best first.
    public List<string> CoverUrls = new();
    public List<string> BackdropUrls = new();
}

/// Milestone 7.1: game details and pictures from the internet.
///
///  * Steam's store: no key. Name suggestions, details, cover and backdrop.
///  * RAWG: the player's own key (Settings). Details for games Steam doesn't sell.
///  * SteamGridDB: the player's own key (Settings). Backdrops and covers.
///
/// DXO stays local-first: nothing here runs unless "Look games up online" is
/// on, and only when the player is adding a game or a game has no backdrop.
/// The text sent out is the game's name. Pictures are downloaded once and
/// kept in DXO's art folder. Every call fails quietly when offline.
public static class GameMetadata
{
    private const string SteamSearch = "https://store.steampowered.com/api/storesearch/?l=english&cc=US&term=";
    private const string SteamDetails = "https://store.steampowered.com/api/appdetails?l=english&appids=";
    private const string SteamArt = "https://cdn.cloudflare.steamstatic.com/steam/apps/";
    private const string Rawg = "https://api.rawg.io/api/games";
    private const string Sgdb = "https://www.steamgriddb.com/api/v2";
    private const int MaxPictureBytes = 20 * 1024 * 1024;

    private static readonly HttpClient Http = MakeClient();

    private static HttpClient MakeClient()
    {
        var c = new HttpClient { Timeout = TimeSpan.FromSeconds(15) };
        c.DefaultRequestHeaders.UserAgent.ParseAdd("DXO/0.3 (+https://github.com/NathanChomba/DXO)");
        return c;
    }

    public static bool Enabled => App.Settings.OnlineGameLookups;
    private static string RawgKey => App.Settings.RawgKey.Trim();
    private static string SgdbKey => App.Settings.SteamGridDbKey.Trim();

    // ================= suggestions =================

    /// Games whose name matches what has been typed so far. Steam first, then
    /// RAWG (if a key is set) for anything Steam didn't list. Empty when
    /// offline, switched off, or the name is too short.
    public static async Task<List<GameSuggestion>> SuggestAsync(string name, CancellationToken ct)
    {
        var list = new List<GameSuggestion>();
        name = name.Trim();
        if (!Enabled || name.Length < 2) return list;

        try
        {
            using var doc = await GetJsonAsync(SteamSearch + Uri.EscapeDataString(name), null, ct);
            if (doc is not null && doc.RootElement.TryGetProperty("items", out var items) && items.ValueKind == JsonValueKind.Array)
            {
                foreach (var it in items.EnumerateArray().Take(6))
                {
                    string n = Str(it, "name");
                    if (n.Length > 0 && it.TryGetProperty("id", out var id))
                        list.Add(new GameSuggestion("Steam", id.ToString(), n, ""));
                }
            }
        }
        catch (Exception ex) when (IsNetwork(ex)) { /* offline: no Steam suggestions */ }

        if (RawgKey.Length > 0 && list.Count < 6)
        {
            try
            {
                string url = $"{Rawg}?key={Uri.EscapeDataString(RawgKey)}&page_size=6&search={Uri.EscapeDataString(name)}";
                using var doc = await GetJsonAsync(url, null, ct);
                if (doc is not null && doc.RootElement.TryGetProperty("results", out var results) && results.ValueKind == JsonValueKind.Array)
                {
                    foreach (var it in results.EnumerateArray())
                    {
                        string n = Str(it, "name");
                        if (n.Length == 0 || !it.TryGetProperty("id", out var id)) continue;
                        if (list.Any(s => SameName(s.Name, n))) continue; // Steam already has it
                        string released = Str(it, "released");
                        list.Add(new GameSuggestion("RAWG", id.ToString(), n, released.Length >= 4 ? released[..4] : ""));
                        if (list.Count >= 8) break;
                    }
                }
            }
            catch (Exception ex) when (IsNetwork(ex)) { }
        }
        return list;
    }

    // ================= details =================

    /// Everything the suggestion's source knows about the game, or null.
    public static async Task<GameDetails?> DetailsAsync(GameSuggestion s, CancellationToken ct)
    {
        if (!Enabled) return null;
        try
        {
            return s.Source == "Steam" && int.TryParse(s.Id, out int appId)
                ? await SteamDetailsAsync(appId, s.Name, ct)
                : await RawgDetailsAsync(s.Id, ct);
        }
        catch (Exception ex) when (IsNetwork(ex)) { return null; }
    }

    private static async Task<GameDetails?> SteamDetailsAsync(int appId, string fallbackName, CancellationToken ct)
    {
        var d = new GameDetails { Source = "Steam", SteamAppId = appId, Name = fallbackName };
        // These two files exist for most games in Steam's library layout; older or
        // very new games may not have them, hence the fallbacks added below.
        d.CoverUrls.Add($"{SteamArt}{appId}/library_600x900.jpg");
        d.BackdropUrls.Add($"{SteamArt}{appId}/library_hero.jpg");

        using var doc = await GetJsonAsync(SteamDetails + appId, null, ct);
        if (doc is null || !doc.RootElement.TryGetProperty(appId.ToString(), out var entry)) return d;
        if (!entry.TryGetProperty("data", out var data) || data.ValueKind != JsonValueKind.Object) return d;

        string name = Str(data, "name");
        if (name.Length > 0) d.Name = name;
        d.Description = Str(data, "short_description");
        if (data.TryGetProperty("genres", out var genres) && genres.ValueKind == JsonValueKind.Array)
            d.Genre = string.Join(", ", genres.EnumerateArray().Select(g => Str(g, "description")).Where(x => x.Length > 0).Take(2));
        if (data.TryGetProperty("developers", out var devs) && devs.ValueKind == JsonValueKind.Array)
            d.Developer = devs.EnumerateArray().Select(x => x.GetString() ?? "").FirstOrDefault(x => x.Length > 0) ?? "";
        if (data.TryGetProperty("release_date", out var rel) && rel.ValueKind == JsonValueKind.Object)
            d.ReleaseDate = Str(rel, "date");

        // Trailer: Steam has served it as a plain video file and, more recently, as a
        // stream; take whichever this game's entry has, plain file first.
        if (data.TryGetProperty("movies", out var movies) && movies.ValueKind == JsonValueKind.Array)
        {
            foreach (var m in movies.EnumerateArray())
            {
                string url = "";
                foreach (string kind in new[] { "mp4", "webm" })
                    if (url.Length == 0 && m.TryGetProperty(kind, out var files) && files.ValueKind == JsonValueKind.Object)
                        url = Str(files, "max").Length > 0 ? Str(files, "max") : Str(files, "480");
                if (url.Length == 0) url = Str(m, "hls_h264");
                if (url.Length == 0) url = Str(m, "dash_h264");
                if (url.StartsWith("http://", StringComparison.OrdinalIgnoreCase)) url = "https://" + url[7..];
                if (url.StartsWith("https://", StringComparison.OrdinalIgnoreCase)) { d.TrailerUrl = url; break; }
            }
        }

        string header = Str(data, "header_image");
        if (data.TryGetProperty("screenshots", out var shots) && shots.ValueKind == JsonValueKind.Array)
        {
            // A few screenshots: with the hero picture they make up to three backdrops.
            d.BackdropUrls.AddRange(shots.EnumerateArray().Select(x => Str(x, "path_full")).Where(x => x.Length > 0).Take(3));
        }
        if (header.Length > 0)
        {
            d.CoverUrls.Add(header);
            d.BackdropUrls.Add(header);
        }
        return d;
    }

    private static async Task<GameDetails?> RawgDetailsAsync(string id, CancellationToken ct)
    {
        if (RawgKey.Length == 0) return null;
        using var doc = await GetJsonAsync($"{Rawg}/{Uri.EscapeDataString(id)}?key={Uri.EscapeDataString(RawgKey)}", null, ct);
        if (doc is null) return null;
        var data = doc.RootElement;
        var d = new GameDetails
        {
            Source = "RAWG",
            Name = Str(data, "name"),
            Description = Shorten(Str(data, "description_raw"), 600),
            ReleaseDate = Str(data, "released"),
        };
        if (data.TryGetProperty("genres", out var genres) && genres.ValueKind == JsonValueKind.Array)
            d.Genre = string.Join(", ", genres.EnumerateArray().Select(g => Str(g, "name")).Where(x => x.Length > 0).Take(2));
        if (data.TryGetProperty("developers", out var devs) && devs.ValueKind == JsonValueKind.Array)
            d.Developer = devs.EnumerateArray().Select(x => Str(x, "name")).FirstOrDefault(x => x.Length > 0) ?? "";
        string main = Str(data, "background_image"), extra = Str(data, "background_image_additional");
        if (main.Length > 0) d.CoverUrls.Add(main);
        if (extra.Length > 0) d.BackdropUrls.Add(extra);
        if (main.Length > 0) d.BackdropUrls.Add(main);
        return d.Name.Length > 0 ? d : null;
    }

    // ================= pictures =================

    /// Download the cover and backdrop for a game from the details the player
    /// picked (plus SteamGridDB, when a key is set). Only fills what is asked
    /// for. Returns true if at least one picture was saved.
    public static async Task<bool> ApplyArtAsync(LibraryGame g, GameDetails d, bool cover, bool backdrop, CancellationToken ct)
    {
        if (!Enabled) return false;
        bool any = false;
        int sgdbId = 0;
        if (SgdbKey.Length > 0 && (cover || backdrop))
        {
            try { sgdbId = await SgdbGameIdAsync(d.SteamAppId, d.Name, ct); }
            catch (Exception ex) when (IsNetwork(ex)) { }
        }
        if (cover)
        {
            var urls = new List<string>(d.CoverUrls);
            // Tiles are box-art shaped (2:3): Steam's own cover first, then a community one of the same shape.
            if (sgdbId != 0) urls.InsertRange(Math.Min(1, urls.Count), await SgdbPicturesAsync("grids", sgdbId, "?dimensions=600x900", ct));
            any |= await SaveFirstAsync(g, urls, backdrop: false, ct);
        }
        if (backdrop)
        {
            var urls = new List<string>(d.BackdropUrls);
            if (sgdbId != 0) urls.InsertRange(Math.Min(1, urls.Count), await SgdbPicturesAsync("heroes", sgdbId, "", ct));
            any |= await AddBackdropsAsync(g, urls, ct);
            g.ExtraBackdropsTried = true;
        }
        return any;
    }

    /// Does this game still need anything looked up online?
    public static bool NeedsLookup(LibraryGame g) =>
        (!g.BackdropTried && (g.Background.Length == 0 || g.Artwork.Length == 0))
        || (!g.ExtraBackdropsTried && GameLibrary.AllBackdrops(g).Count < GameLibrary.MaxBackdrops)
        || (!g.DetailsTried && (g.Description.Length == 0 || g.Genre.Length == 0 || g.ReleaseDate.Length == 0));

    /// Fill in what a game is missing: description, genre, developer, release
    /// date, trailer, cover, backdrop. Works out which game it is from its
    /// name (exact matches only: wrong details are worse than none). Never
    /// overwrites anything already there. Each part is tried once, unless the
    /// PC was offline. Returns true if anything changed.
    public static async Task<bool> FillMissingAsync(LibraryGame g, CancellationToken ct)
    {
        if (!Enabled || g.Name.Trim().Length < 2 || !NeedsLookup(g)) return false;
        try
        {
            GameDetails? d = null;
            if (g.SteamAppId != 0) d = await SteamDetailsAsync(g.SteamAppId, g.Name, ct);
            else
            {
                var hit = (await SuggestAsync(g.Name, ct)).FirstOrDefault(s => SameName(s.Name, g.Name));
                if (hit is not null) d = await DetailsAsync(hit, ct);
            }

            bool changed = false;
            if (d is not null && !g.DetailsTried)
            {
                if (d.SteamAppId != 0 && g.SteamAppId == 0) { g.SteamAppId = d.SteamAppId; changed = true; }
                if (g.Description.Length == 0 && d.Description.Length > 0) { g.Description = d.Description; g.DetailsFrom = d.Source; changed = true; }
                if (g.Genre.Length == 0 && d.Genre.Length > 0) { g.Genre = d.Genre; changed = true; }
                if (g.Developer.Length == 0 && d.Developer.Length > 0) { g.Developer = d.Developer; changed = true; }
                if (g.ReleaseDate.Length == 0 && d.ReleaseDate.Length > 0) { g.ReleaseDate = d.ReleaseDate; changed = true; }
                if (g.TrailerUrl.Length == 0 && d.TrailerUrl.Length > 0) { g.TrailerUrl = d.TrailerUrl; changed = true; }
            }
            g.DetailsTried = true;

            bool needCover = !g.BackdropTried && g.Artwork.Length == 0;
            bool needBackdrops = (!g.BackdropTried && g.Background.Length == 0)
                || (!g.ExtraBackdropsTried && GameLibrary.AllBackdrops(g).Count < GameLibrary.MaxBackdrops);
            if (needCover || needBackdrops)
            {
                // Unknown to Steam and RAWG: SteamGridDB can still search by name.
                d ??= new GameDetails { Name = g.Name };
                changed |= await ApplyArtAsync(g, d, needCover, needBackdrops, ct);
            }
            g.BackdropTried = true;
            g.ExtraBackdropsTried = true;
            return changed;
        }
        catch (Exception ex) when (IsNetwork(ex))
        {
            return false; // offline: try again another day
        }
    }

    /// Download backdrops until the game has three, skipping any picture it
    /// already has (compared by content, so a re-download isn't kept twice).
    private static async Task<bool> AddBackdropsAsync(LibraryGame g, IEnumerable<string> urls, CancellationToken ct)
    {
        var have = new HashSet<string>();
        foreach (string f in GameLibrary.AllBackdrops(g))
        {
            try { have.Add(Hash(await System.IO.File.ReadAllBytesAsync(f, ct))); }
            catch (System.IO.IOException) { }
        }
        bool any = false;
        foreach (string url in urls.Where(u => u.StartsWith("https://", StringComparison.OrdinalIgnoreCase)).Distinct())
        {
            if (GameLibrary.AllBackdrops(g).Count >= GameLibrary.MaxBackdrops) break;
            var (bytes, ext) = await DownloadPictureAsync(url, ct);
            if (bytes is null || !have.Add(Hash(bytes))) continue;
            any |= GameLibrary.AddBackdrop(g, bytes, ext, front: false);
        }
        return any;
    }

    private static string Hash(byte[] data) => Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(data));

    private static async Task<bool> SaveFirstAsync(LibraryGame g, IEnumerable<string> urls, bool backdrop, CancellationToken ct)
    {
        foreach (string url in urls.Where(u => u.StartsWith("https://", StringComparison.OrdinalIgnoreCase)).Distinct())
        {
            var (bytes, ext) = await DownloadPictureAsync(url, ct);
            if (bytes is not null && GameLibrary.SaveArt(g, bytes, ext, backdrop)) return true;
        }
        return false;
    }

    private static async Task<(byte[]? Bytes, string Ext)> DownloadPictureAsync(string url, CancellationToken ct)
    {
        try
        {
            using var resp = await Http.GetAsync(url, HttpCompletionOption.ResponseHeadersRead, ct);
            if (!resp.IsSuccessStatusCode) return (null, "");
            string type = resp.Content.Headers.ContentType?.MediaType ?? "";
            string ext = type switch
            {
                "image/jpeg" or "image/jpg" => ".jpg",
                "image/png" => ".png",
                "image/webp" => ".webp",
                _ => "",
            };
            if (ext.Length == 0) return (null, ""); // not a picture DXO can show
            if (resp.Content.Headers.ContentLength is long len && len > MaxPictureBytes) return (null, "");
            byte[] bytes = await resp.Content.ReadAsByteArrayAsync(ct);
            return bytes.Length is > 0 and <= MaxPictureBytes ? (bytes, ext) : (null, "");
        }
        catch (Exception ex) when (IsNetwork(ex))
        {
            return (null, "");
        }
    }

    // ================= SteamGridDB =================

    /// SteamGridDB's own number for a game: from its Steam number when known,
    /// else its best name match. 0 if it doesn't know the game.
    private static async Task<int> SgdbGameIdAsync(int steamAppId, string name, CancellationToken ct)
    {
        if (steamAppId != 0)
        {
            using var doc = await GetJsonAsync($"{Sgdb}/games/steam/{steamAppId}", SgdbKey, ct);
            if (doc is not null && doc.RootElement.TryGetProperty("data", out var data) && data.ValueKind == JsonValueKind.Object
                && data.TryGetProperty("id", out var id) && id.TryGetInt32(out int n)) return n;
        }
        if (name.Trim().Length >= 2)
        {
            using var doc = await GetJsonAsync($"{Sgdb}/search/autocomplete/{Uri.EscapeDataString(name.Trim())}", SgdbKey, ct);
            if (doc is not null && doc.RootElement.TryGetProperty("data", out var data) && data.ValueKind == JsonValueKind.Array)
            {
                foreach (var it in data.EnumerateArray())
                    if (SameName(Str(it, "name"), name) && it.TryGetProperty("id", out var id) && id.TryGetInt32(out int n)) return n;
            }
        }
        return 0;
    }

    /// Picture addresses of one kind ("heroes" = wide backdrops, "grids" = covers).
    private static async Task<List<string>> SgdbPicturesAsync(string kind, int gameId, string query, CancellationToken ct)
    {
        var urls = new List<string>();
        try
        {
            using var doc = await GetJsonAsync($"{Sgdb}/{kind}/game/{gameId}{query}", SgdbKey, ct);
            if (doc is not null && doc.RootElement.TryGetProperty("data", out var data) && data.ValueKind == JsonValueKind.Array)
                urls.AddRange(data.EnumerateArray().Select(x => Str(x, "url")).Where(u => u.Length > 0).Take(3));
        }
        catch (Exception ex) when (IsNetwork(ex)) { }
        return urls;
    }

    // ================= helpers =================

    /// GET a JSON document; null for "not found" and other refusals.
    private static async Task<JsonDocument?> GetJsonAsync(string url, string? bearer, CancellationToken ct)
    {
        using var req = new HttpRequestMessage(HttpMethod.Get, url);
        if (!string.IsNullOrEmpty(bearer)) req.Headers.Authorization = new AuthenticationHeaderValue("Bearer", bearer);
        using var resp = await Http.SendAsync(req, ct);
        if (!resp.IsSuccessStatusCode) return null;
        try
        {
            return JsonDocument.Parse(await resp.Content.ReadAsStringAsync(ct));
        }
        catch (JsonException)
        {
            return null;
        }
    }

    private static string Str(JsonElement e, string property) =>
        e.ValueKind == JsonValueKind.Object && e.TryGetProperty(property, out var v) && v.ValueKind == JsonValueKind.String
            ? v.GetString() ?? ""
            : "";

    /// "The Witcher 3: Wild Hunt" and "the witcher 3 wild hunt" are the same game.
    private static bool SameName(string a, string b)
    {
        static string Key(string s) => new(s.Where(char.IsLetterOrDigit).Select(char.ToLowerInvariant).ToArray());
        string ka = Key(a);
        return ka.Length > 0 && ka == Key(b);
    }

    private static string Shorten(string s, int max)
    {
        s = s.Trim();
        if (s.Length <= max) return s;
        int cut = s.LastIndexOf(". ", max, StringComparison.Ordinal);
        return cut > max / 2 ? s[..(cut + 1)] : s[..max].TrimEnd() + "...";
    }

    /// Offline, timed out, or cancelled because the player typed another letter.
    private static bool IsNetwork(Exception ex) =>
        ex is HttpRequestException or TaskCanceledException or OperationCanceledException or System.IO.IOException;
}
