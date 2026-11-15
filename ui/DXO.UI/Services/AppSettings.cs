using System;
using System.Diagnostics;
using System.IO;
using System.Security;
using System.Security.Principal;
using System.Text;
using System.Text.Json;
using Microsoft.Win32;

namespace DXO.UI.Services;

/// App-level preferences (not per profile): %LOCALAPPDATA%\DXO\settings.json
public sealed class AppSettings
{
    public bool CloseToTray { get; set; } = true;
    public bool MinimizeToTray { get; set; }
    /// Also use Switch Pro and generic USB/Bluetooth gamepads (experimental).
    public bool IncludeGeneric { get; set; }
    /// Drive the DXO window with the controller (focus, invoke, scroll) while it's in front.
    public bool ControllerNavigation { get; set; } = true;
    /// System chords: hold PS + a button for power-off, volume, screenshot, show/hide.
    public bool SystemChords { get; set; } = true;

    // Animation speed, like Android's developer options: 0 = off, 1 = normal,
    // 0.5 = twice as fast, 2 = twice as slow.
    /// The controller card growing out of a pill and shrinking back.
    public double WindowAnimationScale { get; set; } = 0.3;
    /// Moving between pages.
    public double TransitionAnimationScale { get; set; } = 0.3;
    /// Smaller fades (card contents, dimmed background).
    public double AnimatorDurationScale { get; set; } = 0.3;

    /// The choices offered in Settings, Android-style.
    public static readonly double[] ScaleChoices = { 0, 0.3, 0.5, 1, 1.5, 2, 5, 10 };
    public static readonly string[] ScaleLabels = { "Animation off", "0.3x", "0.5x", "1x", "1.5x", "2x", "5x", "10x" };
    /// The "DXO is still running" tray balloon has been shown once.
    public bool TrayHintShown { get; set; }
    /// First-run setup has been shown (Settings > Run setup shows it again).
    public bool SetupDone { get; set; }
    /// Where to look for new versions: "owner/repo" (public GitHub) or an https JSON address.
    public string UpdateSource { get; set; } = "";
    public bool CheckUpdatesOnStart { get; set; } = true;
    /// "Later" on this version: don't ask again at start-up until a newer one.
    public string UpdateDismissed { get; set; } = "";
    /// PS + Options shows DXO's on-screen keyboard, which types into the app in front.
    public bool OnScreenKeyboard { get; set; } = true;
    /// Size of the on-screen keyboard: 1 = half the screen's width.
    public double KeyboardScale { get; set; } = 1.0;
    /// PS + Options opens Windows 11's own gamepad keyboard instead of DXO's,
    /// for a controller emulated as an Xbox pad. Experimental.
    public bool UseWindowsKeyboard { get; set; }
    /// The system chord table as the engine's JSON; "" = DXO's defaults.
    public string ChordsJson { get; set; } = "";
    /// The quick-menu items as the engine's JSON; "" = DXO's defaults.
    public string QuickMenuJson { get; set; } = "";
    /// Games page: look game details and artwork up online (Steam's store, and
    /// SteamGridDB / RAWG when their keys are filled in). Only while adding a
    /// game or filling in a missing backdrop.
    public bool OnlineGameLookups { get; set; } = true;
    /// The player's own SteamGridDB key ("" = not used). Never leaves this PC except to SteamGridDB.
    public string SteamGridDbKey { get; set; } = "";
    /// The player's own RAWG key ("" = not used).
    public string RawgKey { get; set; } = "";
    /// Warn (a Windows notification) when a controller's battery runs low.
    public bool LowBatteryWarning { get; set; } = true;
    /// Community profile hub: the https address of its index.json. "" = off;
    /// new installs start on DXO's own hub
    /// (DXO contacts it only while the Community page is open).
    public string CommunityHub { get; set; } = OfficialHub;

    /// DXO's own profile repository (github.com/NathanChomba/DXO-Profiles).
    public const string OfficialHub = "https://raw.githubusercontent.com/NathanChomba/DXO-Profiles/main/index.json";

    private static readonly string FilePath = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "DXO", "settings.json");

    private static readonly JsonSerializerOptions Json = new() { WriteIndented = true };

    public static AppSettings Load()
    {
        try
        {
            if (File.Exists(FilePath))
                return JsonSerializer.Deserialize<AppSettings>(File.ReadAllText(FilePath), Json) ?? new();
        }
        catch (Exception ex) { App.LogCrash("AppSettings.Load", ex); }
        return new();
    }

    public void Save()
    {
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(FilePath)!);
            File.WriteAllText(FilePath, JsonSerializer.Serialize(this, Json));
        }
        catch (Exception ex) { App.LogCrash("AppSettings.Save", ex); }
    }
}

/// "Start with Windows". Two mechanisms:
///  * elevated DXO -> a Task Scheduler task with highest privileges, so DXO
///    starts as admin at sign-in and can still hide the controller
///    (Windows silently skips admin apps listed in the Run key);
///  * normal DXO -> the per-user Run key.
/// Both start DXO with --tray (no window, just the tray icon).
public static class Autostart
{
    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string Name = "DXO";

    public static bool IsElevated
    {
        get
        {
            using var id = WindowsIdentity.GetCurrent();
            return new WindowsPrincipal(id).IsInRole(WindowsBuiltInRole.Administrator);
        }
    }

    private static string Exe => Environment.ProcessPath ?? Path.Combine(AppContext.BaseDirectory, "DXO.UI.exe");

    public static bool HasRunKey
    {
        get
        {
            using var k = Registry.CurrentUser.OpenSubKey(RunKey);
            return k?.GetValue(Name) is string;
        }
    }

    public static bool HasTask => Schtasks("/Query /TN \"DXO\"") == 0;

    public static bool IsEnabled => HasRunKey || HasTask;

    /// After moving DXO (e.g. installing it), re-point an old startup entry at this exe.
    public static void RefreshIfMoved()
    {
        if (!IsElevated) return;
        if (HasRunKey) { Set(true); return; } // Run key can't start an admin app: move it to a task
        if (!HasTask) return;
        try
        {
            using var p = Process.Start(new ProcessStartInfo("schtasks.exe", "/Query /TN \"DXO\" /XML")
            {
                CreateNoWindow = true, UseShellExecute = false, RedirectStandardOutput = true,
            })!;
            string xml = p.StandardOutput.ReadToEnd();
            p.WaitForExit(5000);
            if (!xml.Contains(SecurityElement.Escape(Exe), StringComparison.OrdinalIgnoreCase)) Set(true);
        }
        catch (Exception ex) { App.LogCrash("Autostart.RefreshIfMoved", ex); }
    }

    /// Returns an error message, or null on success.
    public static string? Set(bool on)
    {
        try
        {
            if (!on)
            {
                using (var k = Registry.CurrentUser.CreateSubKey(RunKey)) k.DeleteValue(Name, false);
                if (HasTask && Schtasks("/Delete /TN \"DXO\" /F") != 0)
                    return "Couldn't remove the startup task. Run DXO as administrator and try again.";
                return null;
            }

            if (IsElevated)
            {
                var xml = Path.Combine(Path.GetTempPath(), "dxo-task.xml");
                File.WriteAllText(xml, TaskXml(), Encoding.Unicode); // schtasks wants UTF-16
                int rc = Schtasks($"/Create /TN \"DXO\" /XML \"{xml}\" /F");
                try { File.Delete(xml); } catch { }
                if (rc != 0) return $"Task Scheduler refused (code {rc}).";
                using (var k = Registry.CurrentUser.CreateSubKey(RunKey)) k.DeleteValue(Name, false); // one mechanism only
                return null;
            }

            using (var k = Registry.CurrentUser.CreateSubKey(RunKey))
                k.SetValue(Name, $"\"{Exe}\" --tray");
            return null;
        }
        catch (Exception ex)
        {
            App.LogCrash("Autostart.Set", ex);
            return ex.Message;
        }
    }

    private static int Schtasks(string args)
    {
        try
        {
            using var p = Process.Start(new ProcessStartInfo("schtasks.exe", args)
            {
                CreateNoWindow = true, UseShellExecute = false, RedirectStandardOutput = true, RedirectStandardError = true,
            })!;
            p.WaitForExit(5000);
            return p.HasExited ? p.ExitCode : -1;
        }
        catch { return -1; }
    }

    private static string TaskXml()
    {
        string user = SecurityElement.Escape(WindowsIdentity.GetCurrent().Name);
        string exe = SecurityElement.Escape(Exe);
        return $"""
        <?xml version="1.0" encoding="UTF-16"?>
        <Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
          <RegistrationInfo><Description>Start DXO in the tray when you sign in.</Description></RegistrationInfo>
          <Triggers>
            <LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId><Delay>PT5S</Delay></LogonTrigger>
          </Triggers>
          <Principals>
            <Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal>
          </Principals>
          <Settings>
            <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
            <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
            <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
            <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
            <Enabled>true</Enabled>
          </Settings>
          <Actions Context="Author">
            <Exec><Command>{exe}</Command><Arguments>--tray</Arguments></Exec>
          </Actions>
        </Task>
        """;
    }
}
