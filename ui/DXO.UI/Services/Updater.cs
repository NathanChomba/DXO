using System;
using System.Linq;
using System.Net.Http;
using System.Text.Json;
using System.Threading.Tasks;

namespace DXO.UI.Services;

/// "Is there a newer DXO?" Reads one of:
///   * a GitHub repo as owner/name: its latest release (the repo must be public), or
///   * an https address of a small JSON file: {"version": "0.3.0", "url": "https://...", "notes": "..."}
///     (a secret GitHub gist works well while the code itself stays private).
/// Never installs anything itself: it only offers the download link.
public static class Updater
{
    public sealed record Release(string Version, string Url, string Notes);

    /// (newer release or null, error message or null)
    public static async Task<(Release? Release, string? Error)> CheckAsync(string source)
    {
        source = source.Trim();
        if (source.Length == 0) return (null, "Set where DXO looks for updates first.");
        string url = source.StartsWith("https://", StringComparison.OrdinalIgnoreCase)
            ? source
            : $"https://api.github.com/repos/{source.Trim('/')}/releases/latest";
        try
        {
            using var http = new HttpClient { Timeout = TimeSpan.FromSeconds(10) };
            http.DefaultRequestHeaders.UserAgent.ParseAdd($"DXO/{App.Version}");
            var text = await http.GetStringAsync(url);
            using var doc = JsonDocument.Parse(text);
            var r = doc.RootElement;
            string Str(string name) => r.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() ?? "" : "";

            string version = (Str("version") is { Length: > 0 } v1 ? v1 : Str("tag_name")).TrimStart('v', 'V');
            string link = Str("url");
            if (r.TryGetProperty("assets", out var assets) && assets.ValueKind == JsonValueKind.Array)
            {
                // GitHub release: prefer the installer asset, else the release page.
                link = assets.EnumerateArray()
                    .Select(a => a.TryGetProperty("browser_download_url", out var u) ? u.GetString() ?? "" : "")
                    .FirstOrDefault(u => u.EndsWith(".exe", StringComparison.OrdinalIgnoreCase)) ?? "";
                if (link.Length == 0 || link.Contains("api.github.com")) link = Str("html_url");
            }
            if (!link.StartsWith("https://", StringComparison.OrdinalIgnoreCase)) link = "";
            string notes = Str("notes") is { Length: > 0 } n ? n : Str("body");

            if (version.Length == 0) return (null, "The update address didn't say which version is latest.");
            return IsNewer(version, App.Version) ? (new Release(version, link, notes), null) : (null, null);
        }
        catch (HttpRequestException ex) when (ex.StatusCode == System.Net.HttpStatusCode.NotFound)
        {
            return (null, "Nothing found there. A private GitHub repo can't be checked; use a JSON file address instead.");
        }
        catch (Exception ex)
        {
            return (null, $"Couldn't check: {ex.Message}");
        }
    }

    public static bool IsNewer(string candidate, string current)
    {
        static Version? Parse(string s)
        {
            var core = new string(s.TakeWhile(c => char.IsDigit(c) || c == '.').ToArray()).Trim('.');
            if (core.Count(c => c == '.') == 0) core += ".0";
            return Version.TryParse(core, out var v) ? v : null;
        }
        var a = Parse(candidate);
        var b = Parse(current);
        return a is not null && b is not null && a > b;
    }
}
