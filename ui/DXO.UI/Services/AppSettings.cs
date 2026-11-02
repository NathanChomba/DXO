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
    /// The "DXO is still running" tray balloon has been shown once.
    public bool TrayHintShown { get; set; }

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
