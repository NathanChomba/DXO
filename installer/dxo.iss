; DXO installer (Inno Setup 6.3+). Built by scripts\package.ps1, which first
; publishes the app to dist\DXO. To build by hand:  ISCC installer\dxo.iss
;
; The drivers (ViGEmBus, HidHide) are not bundled: DXO's first-run setup
; checks for them and links to their downloads.

#ifndef MyVersion
  #define MyVersion "0.3.1"
#endif
; Which published folder to pack, and the installer's file name. package.ps1
; sets both: the standard build (dist\DXO) and the one with .NET bundled
; (dist\DXO-with-dotnet, "-with-dotnet" in the name).
#ifndef SourceDir
  #define SourceDir "..\dist\DXO"
#endif
#ifndef OutputName
  #define OutputName "DXO-Setup-" + MyVersion
#endif

[Setup]
AppId={{8F3B2C1A-5D4E-4F6A-9B7C-2E1D0A9F8C34}
AppName=DXO
AppVersion={#MyVersion}
AppVerName=DXO {#MyVersion}
AppPublisher=DXO
DefaultDirName={autopf}\DXO
DefaultGroupName=DXO
DisableProgramGroupPage=yes
OutputDir=..\dist
OutputBaseFilename={#OutputName}
SetupIconFile=..\ui\DXO.UI\Assets\dxo.ico
UninstallDisplayIcon={app}\DXO.UI.exe
UninstallDisplayName=DXO
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
WizardStyle=modern
; The running app holds this; setup asks to close DXO before replacing files.
AppMutex=Local\DXO.UI.SingleInstance
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\DXO"; Filename: "{app}\DXO.UI.exe"
Name: "{autodesktop}\DXO"; Filename: "{app}\DXO.UI.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\DXO.UI.exe"; Description: "{cm:LaunchProgram,DXO}"; Flags: nowait postinstall skipifsilent shellexec

[UninstallRun]
; "Start with Windows" is a scheduled task; remove it with the app.
Filename: "{sys}\schtasks.exe"; Parameters: "/Delete /TN DXO /F"; Flags: runhidden; RunOnceId: "RemoveStartupTask"

[Code]
// If DXO crashed or was killed while it was hiding a controller, that
// controller is still hidden from every game. DXO lists the ones it hides in
// %LOCALAPPDATA%\DXO\hidden.txt and clears the list at its next start; when
// it is uninstalled instead, give them back here.
function HidHideCli(): String;
var
  Candidates: array of String;
  I: Integer;
begin
  Result := '';
  SetArrayLength(Candidates, 4);
  Candidates[0] := ExpandConstant('{commonpf64}\Nefarius Software Solutions\HidHide\x64\HidHideCLI.exe');
  Candidates[1] := ExpandConstant('{commonpf64}\Nefarius Software Solutions\HidHide\HidHideCLI.exe');
  Candidates[2] := ExpandConstant('{commonpf64}\Nefarius Software Solutions e.U\HidHide\x64\HidHideCLI.exe');
  Candidates[3] := ExpandConstant('{commonpf64}\HidHide\x64\HidHideCLI.exe');
  for I := 0 to GetArrayLength(Candidates) - 1 do
    if FileExists(Candidates[I]) then
    begin
      Result := Candidates[I];
      Exit;
    end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  ListFile, Cli, Id: String;
  Lines: TArrayOfString;
  I, Code: Integer;
begin
  if CurUninstallStep <> usUninstall then Exit;
  ListFile := ExpandConstant('{localappdata}\DXO\hidden.txt');
  if not LoadStringsFromFile(ListFile, Lines) then Exit;
  Cli := HidHideCli();
  if Cli = '' then Exit;
  for I := 0 to GetArrayLength(Lines) - 1 do
  begin
    Id := Trim(Lines[I]);
    if Id <> '' then
      Exec(Cli, '--dev-unhide "' + Id + '"', '', SW_HIDE, ewWaitUntilTerminated, Code);
  end;
  DeleteFile(ListFile);
end;
