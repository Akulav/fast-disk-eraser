; Fast Disk Eraser - Inno Setup script. Made by Akulav.
; One file installs the app the first time and updates it later:
;   * first run: asks for the install folder (the app AND its settings, history and updates go there)
;   * later runs and in-app updates (/UPDATE): reuse the remembered folder without asking
; Build: iscc /DAppVersion=X.Y.Z /DSourceExe=path\to\FastDiskEraser.exe installer\fast-disk-eraser.iss

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\target\release\FastDiskEraser.exe"
#endif

#define AppName "Fast Disk Eraser"
#define AppExe "FastDiskEraser.exe"
#define SetupName "FastDiskEraser-setup"
; Marks the install folder; the app keeps its data next to it (src/app/install/mod.rs).
#define Marker ".fde-install"

[Setup]
; AppId never changes: installing over an existing copy upgrades it in place, in the same folder.
; The app reads the folder back from this key (src/app/install/windows.rs).
AppId={{1DF3E4F2-0C1C-4D54-B077-35580DCA2AB3}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Akulav
AppPublisherURL=https://github.com/Akulav/fast-disk-eraser
AppSupportURL=https://github.com/Akulav/fast-disk-eraser/issues
AppUpdatesURL=https://github.com/Akulav/fast-disk-eraser/releases
AppCopyright=Made by Akulav
VersionInfoVersion={#AppVersion}
VersionInfoCompany=Akulav
VersionInfoDescription={#AppName} Setup
VersionInfoProductName={#AppName}
; Per-user install: no admin rights, and the remembered folder lives under HKCU where the app looks.
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\FastDiskEraser
; Ask for the folder on the first install only; updates reuse the previous one.
DisableDirPage=auto
UsePreviousAppDir=yes
DirExistsWarning=no
DisableProgramGroupPage=yes
DefaultGroupName={#AppName}
OutputBaseFilename={#SetupName}
OutputDir=..\dist
SetupIconFile=..\assets\icon.ico
WizardImageFile=wizard.png
WizardSmallImageFile=wizard-small.png
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
WizardStyle=modern
CloseApplications=force
RestartApplications=no
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma2/ultra64
SolidCompression=yes
LicenseFile=..\LICENSE

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
english.DeleteData=Also delete Fast Disk Eraser's settings and erase history?%n%nChoose No to keep them for a future reinstall.
english.LaunchApp=Launch Fast Disk Eraser
english.DataNote=The app and everything it saves (settings, history, updates) will be stored in this folder only.

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[InstallDelete]
Type: files; Name: "{app}\FastDiskEraser.old.exe"
Type: files; Name: "{app}\FastDiskEraser.new.exe"

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"; Comment: "Securely erase files, folders and free space - made by Akulav"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"; Comment: "Securely erase files, folders and free space - made by Akulav"; Tasks: desktopicon

[Run]
; Interactive install: the usual "Launch" checkbox.
Filename: "{app}\{#AppExe}"; Parameters: "--installed"; WorkingDir: "{app}"; Description: "{cm:LaunchApp}"; Flags: nowait postinstall skipifsilent
; Silent in-app update (/UPDATE /FROM=<old version>): start the new version again.
Filename: "{app}\{#AppExe}"; Parameters: "--updated-from ""{param:FROM|}"""; WorkingDir: "{app}"; Flags: nowait; Check: IsUpdate

[UninstallDelete]
Type: files; Name: "{app}\{#Marker}"
Type: files; Name: "{app}\FastDiskEraser.old.exe"
Type: files; Name: "{app}\FastDiskEraser.new.exe"
; A downloaded update setup (in-app updates keep it in the app's own folder).
Type: filesandordirs; Name: "{app}\cache\update"

[Code]
function IsUpdate(): Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/UPDATE') = 0 then
      Result := True;
end;

procedure InitializeWizard();
begin
  WizardForm.SelectDirLabel.Caption := WizardForm.SelectDirLabel.Caption + #13#10 + CustomMessage('DataNote');
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    SaveStringToFile(ExpandConstant('{app}\{#Marker}'), '{#AppVersion}', False);
end;

function HasAppData(Dir: String): Boolean;
begin
  Result := FileExists(Dir + '\settings.json') or FileExists(Dir + '\history.json')
    or FileExists(Dir + '\window.ron') or DirExists(Dir + '\cache');
end;

{ Only what the app itself creates (DATA_ENTRIES in src/app/install/mod.rs), never anything else
  the user keeps in that folder. }
procedure DeleteAppData(Dir: String);
begin
  DelTree(Dir + '\cache', True, True, True);
  DeleteFile(Dir + '\window.ron');
  DeleteFile(Dir + '\history.json');
  DeleteFile(Dir + '\settings.json');
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  AppDir: String;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    AppDir := ExpandConstant('{app}');
    { Never delete data when silent. }
    if (not UninstallSilent) and HasAppData(AppDir) then
      if MsgBox(CustomMessage('DeleteData'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
        DeleteAppData(AppDir);
    RemoveDir(AppDir);
  end;
end;
