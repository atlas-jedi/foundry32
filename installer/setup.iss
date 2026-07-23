; Foundry32 — Inno Setup 6 script (x86 app, installs under Program Files (x86)).
; Installs the hub only; tools (MCP Console, ...) are downloaded by the hub into
; %LOCALAPPDATA%, so they are not part of this installer.
#ifndef MyAppVersion
  #define MyAppVersion "1.0.0"
#endif
#ifndef ExeDir
  #define ExeDir "..\target\i686-pc-windows-msvc\release"
#endif
#define MyAppName "Foundry32"
#define MyAppPublisher "Software Imperial"
#define MyAppURL "https://github.com/atlas-jedi/foundry32"
#define MyAppExeName "foundry32.exe"

[Setup]
; New AppId (distinct from MCP Hangar's) — this is a different product, so there
; is no in-place upgrade from a Hangar install; the release notes say to remove
; MCP Hangar first.
AppId={{A7E3F1B2-9C4D-4E8A-B5F6-1D2C3E4F5A6B}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={autopf}\{#MyAppPublisher}\{#MyAppName}
DisableProgramGroupPage=yes
LicenseFile=..\LICENSE
OutputDir=..\dist
OutputBaseFilename=Foundry32-Setup-{#MyAppVersion}-x86
SetupIconFile=..\crates\foundry32\assets\foundry32.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=admin
PrivilegesRequiredOverridesAllowed=dialog commandline

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#ExeDir}\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent
