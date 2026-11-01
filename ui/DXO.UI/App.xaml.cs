using System;
using System.IO;
using Microsoft.UI.Xaml;
using DXO.UI.Native;
using DXO.UI.ViewModels;

namespace DXO.UI;

public partial class App : Application
{
    /// Single engine for the whole app. Null if the DLL failed to load.
    public static DxoEngine? Engine { get; private set; }
    public static string? StartupError { get; private set; }

    /// Shared controller state for the header card and all pages.
    public static DeviceViewModel Device { get; private set; } = null!;

    /// Needed by file pickers in unpackaged apps (they need an HWND).
    public Window? MainWindowRef { get; private set; }

    public App()
    {
        InitializeComponent();
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        try
        {
            var profiles = Path.Combine(AppContext.BaseDirectory, "profiles");
            Engine = new DxoEngine(Directory.Exists(profiles) ? profiles : null);
        }
        catch (Exception ex) when (ex is DllNotFoundException or InvalidOperationException or EntryPointNotFoundException)
        {
            StartupError = $"Engine failed to start: {ex.Message}. Run scripts\\build.ps1 first.";
        }

        MainWindowRef = new MainWindow();
        Device = new DeviceViewModel(MainWindowRef.DispatcherQueue);
        ((MainWindow)MainWindowRef).Start();
        MainWindowRef.Closed += (_, _) => Engine?.Dispose(); // unhide the pad + unplug the virtual one
        MainWindowRef.Activate();
    }
}
