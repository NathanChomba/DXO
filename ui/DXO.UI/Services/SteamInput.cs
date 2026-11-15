using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Text.RegularExpressions;
using Microsoft.Win32;
using DXO.UI.Native;

namespace DXO.UI.Services;

/// Steam Input coexistence. Steam has its own controller remapper; if it is
/// working on the same pad as DXO, inputs get mapped twice (or the game sees
/// two controllers). DXO can't switch Steam's settings, so this only detects
/// the situation and says what to do about it.
public static class SteamInput
{
    /// true/false = what Steam's settings say; null = couldn't tell.
    public sealed record Status(bool Running, bool? PlayStationSupport, bool? XboxSupport);

    private static readonly Regex Ps = new("\"SteamController_PSSupport\"\\s+\"(\\d+)\"", RegexOptions.Compiled);
    private static readonly Regex Xbox = new("\"SteamController_XBoxSupport\"\\s+\"(\\d+)\"", RegexOptions.Compiled);

    public static Status Check()
    {
        bool running = false;
        try
        {
            var procs = Process.GetProcessesByName("steam");
            running = procs.Length > 0;
            foreach (var p in procs) p.Dispose();
        }
        catch { /* can't list processes: treat as not running */ }

        bool? ps = null, xbox = null;
        try
        {
            string? steam = Registry.GetValue(@"HKEY_CURRENT_USER\Software\Valve\Steam", "SteamPath", null) as string;
            string userdata = Path.Combine((steam ?? "").Replace('/', '\\'), "userdata");
            if (!string.IsNullOrEmpty(steam) && Directory.Exists(userdata))
            {
                // The most recently used account's settings are the ones in effect.
                FileInfo? newest = null;
                foreach (var dir in Directory.GetDirectories(userdata))
                {
                    var f = new FileInfo(Path.Combine(dir, "config", "localconfig.vdf"));
                    if (f.Exists && (newest is null || f.LastWriteTimeUtc > newest.LastWriteTimeUtc)) newest = f;
                }
                if (newest is not null && newest.Length < 64_000_000)
                {
                    string text = File.ReadAllText(newest.FullName);
                    if (Ps.Match(text) is { Success: true } m1) ps = m1.Groups[1].Value != "0";
                    if (Xbox.Match(text) is { Success: true } m2) xbox = m2.Groups[1].Value != "0";
                }
            }
        }
        catch (Exception ex) { App.LogCrash("SteamInput.Check", ex); }
        return new Status(running, ps, xbox);
    }

    /// Plain-language advice for the connected controllers, or null if there's
    /// nothing to warn about. `pads` = what DXO is driving right now.
    public static string? Advice(Status s, IReadOnlyList<PadInfo> pads)
    {
        if (!s.Running || pads.Count == 0) return null;
        var lines = new List<string>();

        // 1. The real pad is visible to Steam as well as the virtual one.
        bool exposed = false;
        foreach (var p in pads)
            if (p.VirtualActive && !p.Hidden && p.Kind is 1 or 2 or 3) exposed = true;
        if (exposed && s.PlayStationSupport != false)
            lines.Add("Steam can see your real controller as well as DXO's virtual one, so games started from Steam may get every input twice. " +
                      "Turn on \"Hide from games\" for the controller (click its pill), or turn off PlayStation controller support in Steam > Settings > Controller.");

        // 2. Steam is remapping DXO's own virtual Xbox pad.
        bool virtualXbox = false;
        foreach (var p in pads)
            if (p.VirtualActive && p.Target == 1) virtualXbox = true;
        if (virtualXbox && s.XboxSupport == true)
            lines.Add("Steam Input is switched on for Xbox controllers, and DXO's virtual pad is one: your DXO mapping runs first, then Steam's on top. " +
                      "That's fine if you want both; if buttons act oddly, turn Steam Input off for that game (its Properties > Controller) or in Steam > Settings > Controller.");

        if (lines.Count == 0) return null;
        lines.Add("In Big Picture mode the same applies; DXO's PS-button shortcuts keep working there.");
        return string.Join("\n\n", lines);
    }
}
