using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using DXO.UI.Native;
using DXO.UI.ViewModels;

namespace DXO.UI.Services;

/// Reads and writes profile files in %LOCALAPPDATA%\DXO\profiles through the
/// engine's own parser, and edits the active profile live.
public static class ProfileStore
{
    /// Every readable profile file: (path, model). Bad files are skipped.
    public static List<(string Path, ProfileModel Model)> All()
    {
        var list = new List<(string, ProfileModel)>();
        Directory.CreateDirectory(App.ProfilesDir);
        foreach (var f in Directory.GetFiles(App.ProfilesDir, "*.toml").OrderBy(f => f))
        {
            try
            {
                if (ProfileModel.FromJson(DxoEngine.ProfileFileToJson(f)) is ProfileModel m) list.Add((f, m));
            }
            catch (Exception ex) { App.LogCrash($"ProfileStore {f}", ex); }
        }
        return list;
    }

    /// The file holding the profile with this name, or a new file name for it.
    public static string PathFor(string name)
    {
        foreach (var (path, m) in All())
            if (m.Name == name) return path;
        var safe = new string(name.Select(ch => Path.GetInvalidFileNameChars().Contains(ch) ? '_' : ch).ToArray()).Trim();
        return Path.Combine(App.ProfilesDir, (safe.Length == 0 ? "profile" : safe) + ".toml");
    }

    public static ProfileModel? Active()
    {
        try { return ProfileModel.FromJson(App.Engine?.ActiveProfileJson ?? ""); }
        catch (Exception ex) { App.LogCrash("ProfileStore.Active", ex); return null; }
    }

    /// Change the active profile live (not saved). False if the engine rejected it.
    public static bool ApplyActive(ProfileModel m) => App.Engine?.SetActiveProfileJson(m.ToJson()) == true;

    /// Write a profile to its file and tell the engine to re-read the folder.
    public static bool Save(ProfileModel m, string? path = null)
    {
        if (!DxoEngine.ProfileJsonToFile(m.ToJson(), path ?? PathFor(m.Name))) return false;
        App.Engine?.ReloadProfiles();
        return true;
    }
}
