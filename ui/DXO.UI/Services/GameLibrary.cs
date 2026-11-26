using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text.Json;

namespace DXO.UI.Services;

/// One game in the library: found by the scan, or added by hand. Everything
/// the player can edit (name, genre, description, artwork) lives here.
public sealed class LibraryGame
{
    public string Id { get; set; } = Guid.NewGuid().ToString("N");
    public string Name { get; set; } = "";
    /// File name of the game's program ("eldenring.exe"): what profiles and
    /// the engine match on.
    public string Exe { get; set; } = "";
    /// Full path of that program, when known (set by hand, or found at first launch).
    public string Path { get; set; } = "";
    /// Install folder (from the scan).
    public string Folder { get; set; } = "";
    /// "Steam", "Epic", "GOG" or "Manual".
    public string Source { get; set; } = "Manual";
    public string Genre { get; set; } = "";
    public string Description { get; set; } = "";
    /// Full path of the cover picture (a copy kept in DXO's own folder), or "".
    public string Artwork { get; set; } = "";
    /// Full path of the wide backdrop shown behind the Games page (the chosen one), or "".
    public string Background { get; set; } = "";
    /// Every backdrop DXO has for the game (up to three), chosen one included.
    public List<string> Backgrounds { get; set; } = new();
    /// Fetched extra backdrops (up to three) already, found or not.
    public bool ExtraBackdropsTried { get; set; }
    /// An animated background picked by the player: a GIF or a short video, or "".
    public string AnimatedBackground { get; set; } = "";
    /// Looked for a backdrop online already (found or not), so DXO doesn't ask again every visit.
    public bool BackdropTried { get; set; }
    /// The game's number in the Steam store, when known (0 = unknown).
    public int SteamAppId { get; set; }
    public string Developer { get; set; } = "";
    /// As the source wrote it, e.g. "29 Oct, 2020".
    public string ReleaseDate { get; set; } = "";
    /// Where the details came from: "", "Steam" or "RAWG" (RAWG asks to be credited).
    public string DetailsFrom { get; set; } = "";
    /// Looked for missing details (description, genre...) online already.
    public bool DetailsTried { get; set; }
    /// A trailer the details screen can play, or "".
    public string TrailerUrl { get; set; } = "";
    /// Minutes the game has been running, counted by DXO (PlayTime.cs).
    public int PlayedMinutes { get; set; }
    /// Size of the install folder, and when it was measured.
    public long SizeBytes { get; set; }
    public DateTime? SizeMeasured { get; set; }
    public bool Favourite { get; set; }
    public DateTime? LastPlayed { get; set; }
    /// Removed from the library by the player: kept so a new scan doesn't bring it back.
    public bool Hidden { get; set; }
}

/// The library file (%LOCALAPPDATA%\DXO\library.json) and the cover pictures
/// beside it (%LOCALAPPDATA%\DXO\art).
public static class GameLibrary
{
    private static readonly string Dir = System.IO.Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "DXO");
    private static readonly string FilePath = System.IO.Path.Combine(Dir, "library.json");
    private static readonly string ArtDir = System.IO.Path.Combine(Dir, "art");
    private static readonly JsonSerializerOptions Json = new() { WriteIndented = true };

    private static List<LibraryGame>? _cache;
    private static readonly object CacheLock = new();

    /// The library. Read from disk once; afterwards every caller (the Games
    /// page, the play-time counter) shares the same list, so one can't
    /// overwrite the other's changes when saving. Change it on the UI thread.
    public static List<LibraryGame> Load()
    {
        lock (CacheLock)
        {
            if (_cache is not null) return _cache;
            try
            {
                if (File.Exists(FilePath))
                    _cache = JsonSerializer.Deserialize<List<LibraryGame>>(File.ReadAllText(FilePath), Json);
            }
            catch (Exception ex) { App.LogCrash("GameLibrary.Load", ex); }
            return _cache ??= new();
        }
    }

    /// Where the game is installed: the scanned folder, or the program's own folder.
    public static string InstallFolder(LibraryGame g)
    {
        if (g.Folder.Length > 0 && Directory.Exists(g.Folder)) return g.Folder;
        string? dir = g.Path.Length > 0 ? System.IO.Path.GetDirectoryName(g.Path) : null;
        return dir is not null && Directory.Exists(dir) ? dir : "";
    }

    /// Total size of every file under a folder (slow for big games: run it off the UI thread).
    public static long FolderSize(string folder)
    {
        long total = 0;
        try
        {
            var opts = new EnumerationOptions { RecurseSubdirectories = true, IgnoreInaccessible = true, AttributesToSkip = FileAttributes.ReparsePoint };
            foreach (var f in new DirectoryInfo(folder).EnumerateFiles("*", opts)) total += f.Length;
        }
        catch (Exception ex) { App.LogCrash("GameLibrary.FolderSize", ex); }
        return total;
    }

    public static void Save(List<LibraryGame> games)
    {
        try
        {
            Directory.CreateDirectory(Dir);
            File.WriteAllText(FilePath, JsonSerializer.Serialize(games, Json));
        }
        catch (Exception ex) { App.LogCrash("GameLibrary.Save", ex); }
    }

    /// Add scanned games that aren't in the library yet (matched on the exe
    /// name). Games the player edited or removed are left exactly as they are.
    /// Returns how many were added.
    public static int Merge(List<LibraryGame> games, IEnumerable<FoundGame> found)
    {
        int added = 0;
        foreach (var f in found)
        {
            if (f.Exe.Length == 0) continue;
            var known = games.FirstOrDefault(g => g.Exe.Equals(f.Exe, StringComparison.OrdinalIgnoreCase));
            if (known is not null)
            {
                if (known.Folder.Length == 0) known.Folder = f.Folder; // learn where it is installed
                continue;
            }
            games.Add(new LibraryGame { Name = f.Name, Exe = f.Exe, Folder = f.Folder, Source = f.Source });
            added++;
        }
        return added;
    }

    /// Where the game's program is: the saved path, else a search of its
    /// install folder (remembered once found). Null if it can't be found.
    public static string? ResolveExe(LibraryGame g)
    {
        if (g.Path.Length > 0 && File.Exists(g.Path)) return g.Path;
        if (g.Folder.Length == 0 || g.Exe.Length == 0 || !Directory.Exists(g.Folder)) return null;
        try
        {
            var opts = new EnumerationOptions { RecurseSubdirectories = true, MaxRecursionDepth = 4, IgnoreInaccessible = true };
            string? hit = Directory.EnumerateFiles(g.Folder, g.Exe, opts).FirstOrDefault();
            if (hit is not null) g.Path = hit;
            return hit;
        }
        catch (Exception ex)
        {
            App.LogCrash("GameLibrary.ResolveExe", ex);
            return null;
        }
    }

    /// Most backdrops kept per game.
    public const int MaxBackdrops = 3;

    /// The game's backdrops that still exist on disk, the chosen one first.
    public static List<string> AllBackdrops(LibraryGame g)
    {
        var list = new List<string>();
        if (g.Background.Length > 0 && File.Exists(g.Background)) list.Add(g.Background);
        foreach (string b in g.Backgrounds)
            if (b.Length > 0 && File.Exists(b) && !list.Contains(b, StringComparer.OrdinalIgnoreCase)) list.Add(b);
        return list;
    }

    /// Keep one more backdrop for the game. `front` puts it first and makes it
    /// the chosen one (a picture the player picked); otherwise it joins the end
    /// and is chosen only if the game had none. Beyond three, the oldest
    /// downloaded one is dropped.
    public static bool AddBackdrop(LibraryGame g, byte[] picture, string ext, bool front)
    {
        try
        {
            if (picture.Length == 0) return false;
            Directory.CreateDirectory(ArtDir);
            string dest = System.IO.Path.Combine(ArtDir, $"{g.Id}-wide-{DateTime.UtcNow.Ticks}{ext}");
            File.WriteAllBytes(dest, picture);
            var list = AllBackdrops(g);
            if (front) list.Insert(0, dest);
            else list.Add(dest);
            while (list.Count > MaxBackdrops)
            {
                string drop = list[^1];
                list.RemoveAt(list.Count - 1);
                if (drop.StartsWith(ArtDir, StringComparison.OrdinalIgnoreCase))
                    try { File.Delete(drop); } catch { /* in use: left behind, harmless */ }
            }
            g.Backgrounds = list;
            if (front || g.Background.Length == 0 || !File.Exists(g.Background)) g.Background = list[0];
            return true;
        }
        catch (Exception ex)
        {
            App.LogCrash("GameLibrary.AddBackdrop", ex);
            return false;
        }
    }

    /// Copy an animated background (GIF or short video) the player chose into
    /// DXO's art folder. Returns false if the file isn't a kind DXO can play.
    public static bool ImportAnimated(LibraryGame g, string sourceFile)
    {
        try
        {
            string ext = System.IO.Path.GetExtension(sourceFile).ToLowerInvariant();
            if (ext is not (".gif" or ".mp4" or ".webm" or ".mov" or ".m4v")) return false;
            if (new FileInfo(sourceFile).Length > 500L * 1024 * 1024) return false; // keep it a short loop
            Directory.CreateDirectory(ArtDir);
            string dest = System.IO.Path.Combine(ArtDir, $"{g.Id}-anim-{DateTime.UtcNow.Ticks}{ext}");
            File.Copy(sourceFile, dest, overwrite: true);
            ClearAnimated(g);
            g.AnimatedBackground = dest;
            return true;
        }
        catch (Exception ex)
        {
            App.LogCrash("GameLibrary.ImportAnimated", ex);
            return false;
        }
    }

    public static void ClearAnimated(LibraryGame g)
    {
        string old = g.AnimatedBackground;
        g.AnimatedBackground = "";
        if (old.Length > 0 && old.StartsWith(ArtDir, StringComparison.OrdinalIgnoreCase))
            try { File.Delete(old); } catch { /* playing right now: left behind, harmless */ }
    }

    public static bool IsVideo(string path) =>
        System.IO.Path.GetExtension(path).ToLowerInvariant() is ".mp4" or ".webm" or ".mov" or ".m4v";

    /// Copy a picture the player chose into DXO's art folder and make it the
    /// game's cover, or its backdrop. Returns false if the file couldn't be copied.
    public static bool ImportArt(LibraryGame g, string sourceFile, bool backdrop = false)
    {
        try
        {
            string ext = System.IO.Path.GetExtension(sourceFile).ToLowerInvariant();
            if (ext is not (".png" or ".jpg" or ".jpeg" or ".bmp" or ".gif" or ".webp")) return false;
            return SaveArt(g, File.ReadAllBytes(sourceFile), ext, backdrop);
        }
        catch (Exception ex)
        {
            App.LogCrash("GameLibrary.ImportArt", ex);
            return false;
        }
    }

    /// Keep a picture (chosen by the player or downloaded) in DXO's art
    /// folder as the game's cover or backdrop. `ext` includes the dot.
    public static bool SaveArt(LibraryGame g, byte[] picture, string ext, bool backdrop)
    {
        try
        {
            if (picture.Length == 0) return false;
            if (backdrop) return AddBackdrop(g, picture, ext, front: true);
            Directory.CreateDirectory(ArtDir);
            // A new file name every time, so the app never shows a stale cached picture.
            string kind = "cover";
            string dest = System.IO.Path.Combine(ArtDir, $"{g.Id}-{kind}-{DateTime.UtcNow.Ticks}{ext}");
            File.WriteAllBytes(dest, picture);
            string old = backdrop ? g.Background : g.Artwork;
            if (backdrop) g.Background = dest;
            else g.Artwork = dest;
            if (old.Length > 0 && old.StartsWith(ArtDir, StringComparison.OrdinalIgnoreCase))
            {
                try { File.Delete(old); } catch { /* still open in the page: left behind, harmless */ }
            }
            return true;
        }
        catch (Exception ex)
        {
            App.LogCrash("GameLibrary.SaveArt", ex);
            return false;
        }
    }
}
