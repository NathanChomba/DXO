using System;
using System.IO;
using Microsoft.UI.Xaml;
using DXO.UI.Native;
using DXO.UI.Services;
using DXO.UI.ViewModels;

namespace DXO.UI;

public partial class App : Application
{
    /// Single engine for the whole app. Null if the DLL failed to load.
    public static DxoEngine? Engine { get; private set; }
    public static string? StartupError { get; private set; }

    /// Shared controller state for the header card and all pages.
    public static DeviceViewModel Device { get; private set; } = null!;

    public static AppSettings Settings { get; } = AppSettings.Load();

    /// "0.2.0", from <Version> in DXO.UI.csproj.
    public static string Version { get; } = typeof(App).Assembly.GetName().Version is { } v ? $"{v.Major}.{v.Minor}.{v.Build}" : "0.0.0";

    /// Held for the app's lifetime: one DXO window at a time.
    private static System.Threading.Mutex? _instance;

    /// Needed by file pickers in unpackaged apps (they need an HWND).
    public Window? MainWindowRef { get; private set; }

    /// %LOCALAPPDATA%\DXO\crash.log - every unhandled error lands here.
    public static readonly string CrashLog = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "DXO", "crash.log");

    public App()
    {
        // Hook these BEFORE InitializeComponent so errors in App.xaml are caught too.
        AppDomain.CurrentDomain.UnhandledException += (_, e) => LogCrash("AppDomain", e.ExceptionObject as Exception);
        UnhandledException += (_, e) => { LogCrash("XAML", e.Exception); };
        try
        {
            InitializeComponent();
        }
        catch (Exception ex)
        {
            LogCrash("App.InitializeComponent", ex);
            throw;
        }
    }

    /// User profiles live outside the build folder so rebuilding never wipes them.
    public static readonly string ProfilesDir = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "DXO", "profiles");

    /// First run: seed the user's profiles folder with the bundled examples.
    private static void EnsureProfilesDir()
    {
        Directory.CreateDirectory(ProfilesDir);
        if (Directory.GetFiles(ProfilesDir, "*.toml").Length > 0) return;
        var bundled = Path.Combine(AppContext.BaseDirectory, "profiles");
        if (!Directory.Exists(bundled)) return;
        foreach (var f in Directory.GetFiles(bundled, "*.toml"))
            File.Copy(f, Path.Combine(ProfilesDir, Path.GetFileName(f)), overwrite: false);
    }

    public static void LogCrash(string where, Exception? ex)
    {
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(CrashLog)!);
            File.AppendAllText(CrashLog,
                $"===== {DateTime.Now:yyyy-MM-dd HH:mm:ss} [{where}] =====\n{ex}\n\n");
        }
        catch { /* never let logging crash the crash handler */ }
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        try
        {
            // Already running (maybe hidden in the tray)? Show that one instead.
            _instance = new System.Threading.Mutex(true, @"Local\DXO.UI.SingleInstance", out bool first);
            if (!first)
            {
                TrayIcon.SignalOtherInstance();
                Environment.Exit(0);
                return;
            }
            // DXO always runs as administrator (app.manifest) and Windows won't start such
            // an app from the Run key at sign-in: keep "Start with Windows" a task pointing here.
            System.Threading.Tasks.Task.Run(Autostart.RefreshIfMoved);

            // Started at sign-in: stay in the tray, no window.
            bool toTray = Array.Exists(Environment.GetCommandLineArgs(), a => a.Equals("--tray", StringComparison.OrdinalIgnoreCase));

            try
            {
                EnsureProfilesDir();
                // DXO_NO_VIRTUAL=1 runs the engine read-only (no virtual pad, no HidHide) for debugging.
                bool virt = Environment.GetEnvironmentVariable("DXO_NO_VIRTUAL") != "1";
                Engine = new DxoEngine(ProfilesDir, virt);
                if (Settings.IncludeGeneric) Engine.SetIncludeGeneric(true);
            }
            catch (Exception ex) when (ex is DllNotFoundException or InvalidOperationException or EntryPointNotFoundException)
            {
                StartupError = $"Engine failed to start: {ex.Message}. Run scripts\\build.ps1 first.";
                LogCrash("Engine", ex);
            }

            MainWindowRef = new MainWindow();
            Device = new DeviceViewModel(MainWindowRef.DispatcherQueue);
            ((MainWindow)MainWindowRef).Start();
            MainWindowRef.Closed += (_, _) => Engine?.Dispose(); // unhide the pad + unplug the virtual one
            if (!toTray) MainWindowRef.Activate();
        }
        catch (Exception ex)
        {
            LogCrash("OnLaunched", ex);
            throw;
        }
    }
}
