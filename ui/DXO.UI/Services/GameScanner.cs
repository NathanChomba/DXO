using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text.Json;
using System.Text.RegularExpressions;
using Microsoft.Win32;

namespace DXO.UI.Services;

public sealed record FoundGame(string Name, string Source, string Exe, string Folder);

/// Finds installed games without any launcher API: reads Steam's library
/// manifests and Epic's install manifests straight from disk.
public static class GameScanner
{
    public static List<FoundGame> Scan()
    {
        var games = new List<FoundGame>();
        try { games.AddRange(Steam()); } catch (Exception ex) { App.LogCrash("GameScanner.Steam", ex); }
        try { games.AddRange(Epic()); } catch (Exception ex) { App.LogCrash("GameScanner.Epic", ex); }
        return games
            .Where(g => g.Exe.Length > 0)
            .GroupBy(g => g.Exe, StringComparer.OrdinalIgnoreCase).Select(g => g.First())
            .OrderBy(g => g.Name, StringComparer.CurrentCultureIgnoreCase).ToList();
    }

    // ---- Steam ----

    private static readonly Regex VdfPath = new("\"path\"\\s+\"([^\"]+)\"", RegexOptions.Compiled);
    private static readonly Regex AcfName = new("\"name\"\\s+\"([^\"]+)\"", RegexOptions.Compiled);
    private static readonly Regex AcfDir = new("\"installdir\"\\s+\"([^\"]+)\"", RegexOptions.Compiled);

    /// Tools that show up as Steam "apps" but aren't games.
    private static readonly string[] SteamSkip =
        { "Steamworks Common Redistributables", "Steam Linux Runtime", "Proton", "SteamVR", "Wallpaper Engine" };

    private static IEnumerable<FoundGame> Steam()
    {
        string? steam = Registry.GetValue(@"HKEY_CURRENT_USER\Software\Valve\Steam", "SteamPath", null) as string
                     ?? Registry.GetValue(@"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath", null) as string;
        if (string.IsNullOrEmpty(steam)) yield break;
        steam = steam.Replace('/', '\\');

        var libraries = new List<string> { steam };
        string vdf = Path.Combine(steam, "steamapps", "libraryfolders.vdf");
        if (File.Exists(vdf))
            foreach (Match m in VdfPath.Matches(File.ReadAllText(vdf)))
                libraries.Add(m.Groups[1].Value.Replace(@"\\", @"\"));

        foreach (var lib in libraries.Distinct(StringComparer.OrdinalIgnoreCase))
        {
            string apps = Path.Combine(lib, "steamapps");
            if (!Directory.Exists(apps)) continue;
            foreach (var acf in Directory.GetFiles(apps, "appmanifest_*.acf"))
            {
                string text;
                try { text = File.ReadAllText(acf); } catch { continue; }
                var name = AcfName.Match(text).Groups[1].Value;
                var dir = AcfDir.Match(text).Groups[1].Value;
                if (name.Length == 0 || dir.Length == 0 || SteamSkip.Any(s => name.StartsWith(s, StringComparison.OrdinalIgnoreCase)))
                    continue;
                string folder = Path.Combine(apps, "common", dir);
                if (!Directory.Exists(folder)) continue;
                yield return new FoundGame(name, "Steam", GuessExe(folder), folder);
            }
        }
    }

    // ---- Epic ----

    private static IEnumerable<FoundGame> Epic()
    {
        string manifests = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
                                        "Epic", "EpicGamesLauncher", "Data", "Manifests");
        if (!Directory.Exists(manifests)) yield break;
        foreach (var item in Directory.GetFiles(manifests, "*.item"))
        {
            FoundGame? g = null;
            try
            {
                using var doc = JsonDocument.Parse(File.ReadAllText(item));
                var r = doc.RootElement;
                string name = r.TryGetProperty("DisplayName", out var n) ? n.GetString() ?? "" : "";
                string folder = r.TryGetProperty("InstallLocation", out var l) ? l.GetString() ?? "" : "";
                string launch = r.TryGetProperty("LaunchExecutable", out var x) ? x.GetString() ?? "" : "";
                if (name.Length > 0 && Directory.Exists(folder))
                    g = new FoundGame(name, "Epic", launch.Length > 0 ? Path.GetFileName(launch) : GuessExe(folder), folder);
            }
            catch { /* skip broken manifests */ }
            if (g is not null) yield return g;
        }
    }

    // ---- exe guess ----

    private static readonly string[] NotTheGame =
    {
        "unins", "crash", "redist", "setup", "install", "vcredist", "vc_redist", "dxsetup", "directx",
        "easyanticheat", "beservice", "battleye", "prereq", "helper", "report", "cef", "update", "dotnet",
        "launcherpatcher", "touchup", "cleanup", "uploader", "benchmark",
    };

    /// Largest .exe in the top three folder levels that doesn't look like a helper.
    public static string GuessExe(string folder)
    {
        var best = ("", 0L);
        void Walk(string dir, int depth)
        {
            string[] files;
            try { files = Directory.GetFiles(dir, "*.exe"); } catch { return; }
            foreach (var f in files)
            {
                var n = Path.GetFileName(f).ToLowerInvariant();
                if (NotTheGame.Any(n.Contains)) continue;
                long size;
                try { size = new FileInfo(f).Length; } catch { continue; }
                if (size > best.Item2) best = (Path.GetFileName(f), size);
            }
            if (depth >= 3) return;
            string[] dirs;
            try { dirs = Directory.GetDirectories(dir); } catch { return; }
            foreach (var d in dirs)
            {
                var dn = Path.GetFileName(d).ToLowerInvariant();
                if (dn is "_commonredist" or "redist" or "redistributables" or "directx" or "support" or "__installer") continue;
                Walk(d, depth + 1);
            }
        }
        Walk(folder, 0);
        return best.Item1;
    }
}
