; Instalador Windows do Arcadia Native (Inno Setup 6).
; Os caminhos e a versão chegam por variáveis de ambiente definidas em
; scripts/package-native-windows-setup.ps1.

#define MyVersion GetEnv("ARCADIA_VERSION")
#define MyStageDir GetEnv("ARCADIA_STAGE_DIR")
#define MyOutputDir GetEnv("ARCADIA_OUTPUT_DIR")
#define MyIcon GetEnv("ARCADIA_ICON")
#define MyLauncher GetEnv("ARCADIA_LAUNCHER")

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
UninstallDisplayIcon={app}\arcadia-native.exe
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
Source: "{#MyStageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
; Substitui o launcher portátil pelo launcher instalado (ARCADIA_PACKAGED=1).
Source: "{#MyLauncher}"; DestDir: "{app}"; DestName: "run-arcadia-native.cmd"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Arcadia"; Filename: "{app}\run-arcadia-native.cmd"; IconFilename: "{app}\arcadia-native.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\Arcadia"; Filename: "{app}\run-arcadia-native.cmd"; IconFilename: "{app}\arcadia-native.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\run-arcadia-native.cmd"; Description: "{cm:LaunchProgram,Arcadia}"; Flags: nowait postinstall skipifsilent
