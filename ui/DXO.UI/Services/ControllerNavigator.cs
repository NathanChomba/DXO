using System;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml.Input;
using DXO.UI.Native;

namespace DXO.UI.Services;

/// Drives the DXO window with the connected controller — focus movement, invoke,
/// back, page/tab flipping and scrolling — entirely in-process (no virtual pad,
/// no injection), so it works even when the pad is emulated as nothing.
///
/// Active only while the DXO window is foreground and the Settings toggle is on.
/// Reads RAW buttons, so a profile's remaps never change how you navigate.
public sealed class ControllerNavigator
{
    /// A screen that captures a raw button press (e.g. a future "press to map")
    /// sets this so navigation doesn't swallow the press.
    public static bool Suspended { get; set; }

    private readonly MainWindow _win;
    private readonly DispatcherQueueTimer _timer;

    private uint _prev;          // buttons last tick, for edge detection
    private int _dir;            // held focus direction (0 none, 1 up, 2 down, 3 left, 4 right)
    private DateTime _nextRepeat;
    private bool _active;        // DXO window is foreground
    private bool _afterKeyboard; // the on-screen keyboard just closed: wait for the buttons to be let go

    /// The player has driven the window with the controller (so pages put
    /// focus somewhere sensible when they open, instead of leaving it nowhere).
    public bool UsingPad { get; private set; }

    // Key-repeat feel, and a stick deadzone with hysteresis so it doesn't chatter.
    private static readonly TimeSpan FirstDelay = TimeSpan.FromMilliseconds(350);
    private static readonly TimeSpan RepeatEvery = TimeSpan.FromMilliseconds(120);
    private const float StickOn = 0.6f, StickOff = 0.4f, ScrollGate = 0.3f;

    public ControllerNavigator(MainWindow win, DispatcherQueue dq)
    {
        _win = win;
        _timer = dq.CreateTimer();
        _timer.Interval = TimeSpan.FromMilliseconds(30);
        _timer.Tick += (_, _) => Tick();
        _timer.Start();
    }

    public void SetWindowActive(bool active)
    {
        _active = active;
        if (!active) { _prev = 0; _dir = 0; } // dropped focus: forget held state
    }

    private void Tick()
    {
        if (!_active || Suspended || !App.Settings.ControllerNavigation || App.Engine is not { } eng)
        {
            _prev = 0; _dir = 0;
            _win.NavSetCue(false);
            return;
        }
        // The on-screen keyboard has the pad: its D-pad and Cross are for keys, not for DXO.
        if (eng.OskOpen || eng.GetRadial().Open != 0) { _afterKeyboard = true; _prev = 0; _dir = 0; return; }
        var n = eng.GetNav();
        if (n.Connected == 0) { _prev = 0; _dir = 0; _win.NavSetCue(false); return; }
        if (_afterKeyboard)
        {
            // Circle closed the keyboard and is still down: it must not also be "back".
            if (n.Buttons != 0) return;
            _afterKeyboard = false;
        }
        // A controller could drive the window but nothing is highlighted: say how to start.
        _win.NavSetCue(!_win.NavHasFocus);

        uint b = n.Buttons;
        // PS is the chord modifier: while it's held, these buttons drive the
        // chord layer (volume, screenshot...), not UI navigation. Bow out so we
        // don't also move focus. (_prev = b so no edge fires when PS releases.)
        if (App.Settings.SystemChords && (b & (uint)DxoButtons.Home) != 0) { _prev = b; _dir = 0; return; }
        bool Edge(DxoButtons bit) => (b & (uint)bit) != 0 && (_prev & (uint)bit) == 0;
        bool Down(DxoButtons bit) => (b & (uint)bit) != 0;

        // ---- directional focus: D-pad first, then left stick ----
        int dir = 0;
        if (Down(DxoButtons.Up)) dir = 1;
        else if (Down(DxoButtons.Down)) dir = 2;
        else if (Down(DxoButtons.Left)) dir = 3;
        else if (Down(DxoButtons.Right)) dir = 4;
        else
        {
            float gate = _dir == 0 ? StickOn : StickOff;
            if (MathF.Max(MathF.Abs(n.Lx), MathF.Abs(n.Ly)) >= gate)
                dir = MathF.Abs(n.Lx) > MathF.Abs(n.Ly) ? (n.Lx > 0 ? 4 : 3) : (n.Ly > 0 ? 1 : 2); // +Y = up
        }

        if (dir != 0 || (b & ~_prev) != 0) UsingPad = true;

        var now = DateTime.UtcNow;
        if (dir == 0) _dir = 0;
        else if (dir != _dir) { _dir = dir; _nextRepeat = now + FirstDelay; Move(dir); }
        else if (now >= _nextRepeat) { _nextRepeat = now + RepeatEvery; Move(dir); }

        // ---- buttons (edge-triggered) ----
        if (Edge(DxoButtons.South)) _win.NavActivate();   // Cross / A
        if (Edge(DxoButtons.East)) _win.NavBack();        // Circle / B
        if (Edge(DxoButtons.L1)) _win.NavCycle(-1);       // previous page / tab
        if (Edge(DxoButtons.R1)) _win.NavCycle(+1);       // next page / tab
        if (Edge(DxoButtons.Start)) _win.NavTogglePane(); // Options: the page's own details (Games); the menu stays collapsed
        if (Edge(DxoButtons.Home)) _win.NavHome();        // PS: Home

        // ---- right stick scroll ----
        if (MathF.Abs(n.Ry) > ScrollGate) _win.NavScroll(-n.Ry * 28); // +Y up = scroll up

        _prev = b;
    }

    private void Move(int dir) => _win.NavDirection(dir switch
    {
        1 => FocusNavigationDirection.Up,
        2 => FocusNavigationDirection.Down,
        3 => FocusNavigationDirection.Left,
        _ => FocusNavigationDirection.Right,
    });
}
