# Windows PowerShell 5.1's existing .NET compiler creates a private test exe.
# No tool installation, user configuration, services or Herdr commands.
param([Parameter(Mandatory)][string]$Output)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$schema = Get-Content -LiteralPath (Join-Path $repo 'herdr-app/binding-schema.json') -Raw
$source = @'
using System;
public class InstallerFixture {
    public static int Main(string[] args) {
        var log = Environment.GetEnvironmentVariable("HN_COMMAND_LOG");
        if (log != null) System.IO.File.AppendAllText(log, Environment.GetEnvironmentVariable("HERDR_APP_HOME") + " " + String.Join(" ", args) + "\n");
        if (args.Length == 0) { Console.Error.WriteLine("usage: herdr-nvim <fixture>"); return 2; }
        if (args[0] == "app-core-binding") {
            var supplied = Environment.GetEnvironmentVariable("HN_BINDING_JSON");
            if (supplied != null) { Console.WriteLine(System.IO.File.ReadAllText(supplied)); return 0; }
            var serializer = new System.Web.Script.Serialization.JavaScriptSerializer();
            var schema = serializer.Deserialize<System.Collections.Generic.Dictionary<string,object>>(@"SCHEMA_JSON");
            var selectors = new System.Collections.Generic.Dictionary<string,object>();
            foreach (string name in (System.Collections.IEnumerable)schema["selectors"]) selectors[name] = Environment.GetEnvironmentVariable(name);
            Console.WriteLine(serializer.Serialize(new {
                version = schema["version"], hc_bin = System.Reflection.Assembly.GetExecutingAssembly().Location,
                selectors = selectors, config_path = System.IO.Path.Combine(Environment.GetEnvironmentVariable("HERDR_APP_HOME"), "fixture-config-unused"),
                config_fingerprint = (string)null,
                app_home_identity = new {volume_serial = "0000000000000001", directory_id = "00000000000000000000000000000001"}
            })); return 0;
        }
        // Transaction stand-in only; real file-ID/case-sensitive behavior is
        // tested by windows_app_safety with the native binary, not this mock.
        if (args[0] == "app-home-check") {
            var exe = System.Reflection.Assembly.GetExecutingAssembly().Location;
            var json = System.IO.File.ReadAllText(System.IO.Path.ChangeExtension(exe, "app.json"));
            var data = new System.Web.Script.Serialization.JavaScriptSerializer().Deserialize<System.Collections.Generic.Dictionary<string,object>>(json);
            return args.Length == 2 && String.Equals((string)data["app_home"], args[1], StringComparison.OrdinalIgnoreCase) ? 0 : 1;
        }
        if (args[0] == "check" && Environment.GetEnvironmentVariable("HN_FAIL_CHECK") != null) return 1;
        if (args[0] == "app-setup" && args.Length > 1 && args[1] == "install" && Environment.GetEnvironmentVariable("HN_FAIL_SETUP") != null) return 1;
        if (args[0] == "daemons" && Environment.GetEnvironmentVariable("HN_DIRTY") != null) { Console.Error.WriteLine("unsaved work kept"); return 1; }
        Console.WriteLine("{}");
        return 0;
    }
}
'@
$source = $source.Replace('SCHEMA_JSON', $schema.Replace('"', '""'))
Add-Type -OutputAssembly $Output -OutputType ConsoleApplication -ReferencedAssemblies 'System.Web.Extensions' -TypeDefinition $source
