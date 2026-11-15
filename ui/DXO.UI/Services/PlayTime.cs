using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using System.Threading.Tasks;
using Microsoft.UI.Dispatching;

namespace DXO.UI.Services;

/// Counts how long each game in the library runs, however it was started
/// (from DXO, Steam or a desktop shortcut). Once a minute it looks at which
/// programs are running; each library game found running gets one more
/// minute and "last played" moves to now. Time from before DXO counted
/// starts at zero.
public static class PlayTime
{
    private static DispatcherQueueTimer? _timer;
    private static bool _busy;

    public static void Start(DispatcherQueue dq)
    {
        if (_timer is not null) return;
        _timer = dq.CreateTimer();
        _timer.Interval = TimeSpan.FromMinutes(1);
        _timer.Tick += async (_, _) => await TickAsync();
        _timer.Start();
    }

    private static async Task TickAsync()
    {
        if (_busy) return;
        _busy = true;
        try
        {
            var games = GameLibrary.Load();
            if (games.Count == 0) return;
            // Listing processes takes a moment: do it off the UI thread.
            var running = await Task.Run(RunningProgramNames);
            bool changed = false;
            foreach (var g in games)
            {
                if (g.Hidden || g.Exe.Length == 0) continue;
                if (!running.Contains(System.IO.Path.GetFileNameWithoutExtension(g.Exe))) continue;
                g.PlayedMinutes++;
                g.LastPlayed = DateTime.Now;
                changed = true;
            }
            if (changed) GameLibrary.Save(games);
        }
        catch (Exception ex) { App.LogCrash("PlayTime", ex); }
        finally { _busy = false; }
    }

    private static HashSet<string> RunningProgramNames()
    {
        var names = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var p in Process.GetProcesses())
        {
            try { names.Add(p.ProcessName); }
            catch { /* it exited while we looked */ }
            finally { p.Dispose(); }
        }
        return names;
    }

    /// "57h 47m", "12m", or "Not yet" for the details screen.
    public static string Format(int minutes) =>
        minutes <= 0 ? "Not yet" : minutes < 60 ? $"{minutes}m" : $"{minutes / 60}h {minutes % 60}m";
}
