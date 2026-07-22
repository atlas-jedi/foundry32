; MCP Hangar — Inno Setup 6 script (x86 app, installs under Program Files (x86))
#ifndef MyAppVersion
  #define MyAppVersion "1.0.0"
#endif
#ifndef ExeDir
  #define ExeDir "..\target\i686-pc-windows-msvc\release"
#endif
#define MyAppName "MCP Hangar"
#define MyAppPublisher "Software Imperial"
#define MyAppURL "https://github.com/atlas-jedi/mcp-hangar"
#define MyAppExeName "mcp-hangar.exe"

[Setup]
AppId={{D3F8A2C1-7B4E-4E9A-9C5D-2A6F81B0E437}
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
OutputBaseFilename=MCP-Hangar-Setup-{#MyAppVersion}-x86
SetupIconFile=..\assets\hangar.ico
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
