using System;
using Microsoft.UI.Dispatching;
using DXO.UI.Native;

namespace DXO.UI.Overlay;

/// Shows and hides DXO's overlay windows as the engine opens and closes
/// them: the on-screen keyboard (PS + Options, Circle, a text box in DXO)
/// and the quick menu (PS + R3). Keeps the picture up to date while one is
/// open. Runs for the app's whole life, including while DXO sits in the tray.
public sealed class OnScreenKeyboard
{
    private static readonly TimeSpan Idle = TimeSpan.FromMilliseconds(60);
    private static readonly TimeSpan Live = TimeSpan.FromMilliseconds(16);

    private readonly DispatcherQueueTimer _timer;
    private KeyboardOverlay? _keyboard;
    private RadialOverlay? _menu;
    private bool _keyboardShown, _menuShown;
    private double _scale;
    private bool _errorLogged;

    /// The keyboard or quick menu was asked for but the engine refused: a
    /// game is in front and the controller is in Native mode.
    public event Action? Refused;

    public OnScreenKeyboard(DispatcherQueue dq)
    {
        _timer = dq.CreateTimer();
        _timer.Interval = Idle;
        _timer.Tick += (_, _) => Tick();
        _timer.Start();
    }

    private void Tick()
    {
        try
        {
            if (App.Engine is not { } engine) return;
            if (engine.TakeOskRefused()) Refused?.Invoke();
            // Settings opt-in: Windows' own keyboard, for a pad emulated as Xbox.
            if (engine.TakeOskExternalToggle() && !TouchKeyboard.Toggle())
                engine.SetOskOpen(true); // Windows wouldn't show its keyboard: use DXO's

            // ---- keyboard ----
            DxoOsk o = engine.GetOsk();
            bool open = o.Open != 0;
            if (open && !_keyboardShown)
            {
                _keyboard ??= new KeyboardOverlay(); // built the first time it is needed
                _keyboard.SetHints(KindOf(engine, o.Owner));
                _keyboard.Update(o, engine);
                _scale = App.Settings.KeyboardScale;
                _keyboard.Place(_scale);
                _keyboard.ShowNoActivate();
                _keyboardShown = true;
            }
            else if (!open && _keyboardShown)
            {
                _keyboard?.HideOverlay();
                _keyboardShown = false;
            }
            else if (open && _keyboard is not null)
            {
                if (_scale != App.Settings.KeyboardScale) // the size slider moved
                {
                    _scale = App.Settings.KeyboardScale;
                    _keyboard.Place(_scale);
                }
                _keyboard.Update(o, engine);
            }

            // ---- quick menu ----
            DxoRadial r = engine.GetRadial();
            bool menu = r.Open != 0;
            if (menu && !_menuShown)
            {
                _menu ??= new RadialOverlay();
                _menu.Update(r, engine);
                _menu.Place(App.Settings.KeyboardScale);
                _menu.ShowNoActivate();
                _menuShown = true;
            }
            else if (!menu && _menuShown)
            {
                _menu?.Update(r, engine); // one last paint so the next opening starts clean
                _menu?.HideOverlay();
                _menuShown = false;
            }
            else if (menu) _menu?.Update(r, engine);

            var want = _keyboardShown || _menuShown ? Live : Idle;
            if (_timer.Interval != want) _timer.Interval = want;
        }
        catch (Exception ex)
        {
            if (!_errorLogged) App.LogCrash("Overlay", ex); // once, not 60 times a second
            _errorLogged = true;
        }
    }

    private static byte KindOf(DxoEngine engine, byte slot)
    {
        foreach (var p in engine.ListPads())
            if (p.Slot == slot) return p.Kind;
        return 0;
    }

    /// The app is closing: a hidden window left behind would keep it running.
    public void Shutdown()
    {
        _timer.Stop();
        try { _keyboard?.Close(); } catch { }
        try { _menu?.Close(); } catch { }
        _keyboard = null;
        _menu = null;
    }
}
