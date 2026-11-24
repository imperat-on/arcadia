; Instalador Windows do Arcadia Native (Inno Setup 6).
; Os caminhos e a versão chegam por variáveis de ambiente definidas em
; scripts/package-native-windows-setup.ps1.

#define MyVersion GetEnv("ARCADIA_VERSION")
#define MyStageDir GetEnv("ARCADIA_STAGE_DIR")
#define MyOutputDir GetEnv("ARCADIA_OUTPUT_DIR")
#define MyIcon GetEnv("ARCADIA_ICON")
#define MyInstalledMarker GetEnv("ARCADIA_INSTALLED_MARKER")

[Setup]
AppId={{583a8669-975c-4932-8bc6-fa6f01e6270c}
AppName=Arcadia
AppVersion={#MyVersion}
AppVerName=Arcadia {#MyVersion}
AppPublisher=Arcadia
AppPublisherURL=https://github.com/imperat-on/arcadia
AppSupportURL=https://github.com/imperat-on/arcadia/issues
AppUpdatesURL=https://github.com/imperat-on/arcadia/releases
DefaultDirName={localappdata}\Arcadia
DefaultGroupName=Arcadia
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#MyOutputDir}
OutputBaseFilename=Arcadia_{#MyVersion}_x64-setup
SetupIconFile={#MyIcon}
UninstallDisplayIcon={app}\Arcadia.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
CloseApplications=force
RestartApplications=no
MinVersion=10.0

[Languages]
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; O launcher nativo (Arcadia.exe) já vem do estágio portátil.
Source: "{#MyStageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs; Excludes: "run-arcadia-native.cmd"
; installed.marker faz o launcher definir ARCADIA_PACKAGED=1 (canal NSIS).
Source: "{#MyInstalledMarker}"; DestDir: "{app}"; DestName: "installed.marker"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Arcadia"; Filename: "{app}\Arcadia.exe"; IconFilename: "{app}\Arcadia.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\Arcadia"; Filename: "{app}\Arcadia.exe"; IconFilename: "{app}\Arcadia.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\Arcadia.exe"; Description: "{cm:LaunchProgram,Arcadia}"; Flags: nowait postinstall skipifsilent
