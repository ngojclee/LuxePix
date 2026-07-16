#ifndef MyAppVersion
  #define MyAppVersion "1.1.0"
#endif

#ifndef MyOutputDir
  #define MyOutputDir "..\dist"
#endif

#define MyAppName "LuxePix"
#define MyAppPublisher "Luxe Nail"
#define MyAppExeName "LuxePix.exe"

[Setup]
AppId={{FB281026-7847-4149-A551-8208EC55E64E}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL=https://github.com/ngojclee/LuxePix
AppSupportURL=https://github.com/ngojclee/LuxePix/issues
DefaultDirName={autopf}\LuxePix
DefaultGroupName=LuxePix
DisableProgramGroupPage=yes
PrivilegesRequired=admin
PrivilegesRequiredOverridesAllowed=dialog commandline
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
OutputDir={#MyOutputDir}
OutputBaseFilename=LuxePix-v{#MyAppVersion}-windows-x64-setup
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
CloseApplications=force
RestartApplications=no
UsePreviousAppDir=yes
ChangesEnvironment=no
ChangesAssociations=no
VersionInfoVersion={#MyAppVersion}.0
VersionInfoCompany={#MyAppPublisher}
VersionInfoDescription=LuxePix image and product file explorer
VersionInfoProductName={#MyAppName}
VersionInfoProductVersion={#MyAppVersion}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "..\target\release\luxepix.exe"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Flags: ignoreversion restartreplace
Source: "..\target\release\LuxePixUpdater.exe"; DestDir: "{app}"; Flags: ignoreversion restartreplace
Source: "..\packaging\THIRD_PARTY_NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\LuxePix"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\LuxePix"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Open LuxePix"; Flags: nowait postinstall skipifsilent; Check: not IsUpdateMode

[Code]
function IsUpdateMode: Boolean;
begin
  Result := CompareText(ExpandConstant('{param:LUXEPIXUPDATE|0}'), '1') = 0;
end;

function InitializeSetup: Boolean;
begin
  Result := True;
  if IsUpdateMode and not WizardSilent then
  begin
    Result :=
      MsgBox(
        'LuxePix Setup will close and replace the previous installed version. ' +
        'Your settings, cache, and product index in AppData will be preserved.'#13#10#13#10 +
        'Continue with the update?',
        mbConfirmation,
        MB_YESNO or MB_DEFBUTTON2
      ) = IDYES;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  ResultCode: Integer;
begin
  if (CurStep = ssPostInstall) and IsUpdateMode then
  begin
    Exec(
      ExpandConstant('{app}\{#MyAppExeName}'),
      '',
      ExpandConstant('{app}'),
      SW_SHOWNORMAL,
      ewNoWait,
      ResultCode
    );
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
  begin
    if SuppressibleMsgBox(
      'Remove LuxePix settings, thumbnail cache, and product index from AppData?'#13#10#13#10 +
      'Choose No to keep them for a future installation.',
      mbConfirmation,
      MB_YESNO or MB_DEFBUTTON2,
      IDNO
    ) = IDYES then
    begin
      DelTree(ExpandConstant('{userappdata}\LuxePix'), True, True, True);
    end;
  end;
end;
