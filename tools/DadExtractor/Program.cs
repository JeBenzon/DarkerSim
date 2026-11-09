// DadExtractor - headless Dark and Darker data exporter built on CUE4Parse.
// Reads the game's IoStore containers (read-only, offline) and writes every asset
// under a prefix as JSON. Never touches the running game.
//
// Usage:
//   DadExtractor export <paksDir> <UE5_x> <usmap> <outDir> <prefix> [<prefix> ...]
//   DadExtractor list   <paksDir> <UE5_x> <outFile>
//   DadExtractor probe  <paksDir> <UE5_x> <usmap> <assetPath>
//   DadExtractor raw    <paksDir> <UE5_x> <assetPath> <outFile>
//   DadExtractor artmap <paksDir> <UE5_x> <usmap> <outFile>   (item asset -> art data -> icon texture, TSV)
//   DadExtractor icons  <paksDir> <UE5_x> <usmap> <outDir> [maxSize] [webp|png] [nameFilter]   (item icons -> <outDir>/<Name>.webp)
using System.Collections.Concurrent;
using CUE4Parse.Compression;
using CUE4Parse.UE4.Assets.Exports.Texture;
using CUE4Parse_Conversion;
using CUE4Parse_Conversion.Options;
using CUE4Parse_Conversion.Textures;
using SkiaSharp;
using CUE4Parse.Encryption.Aes;
using CUE4Parse.FileProvider;
using CUE4Parse.MappingsProvider.Usmap;
using CUE4Parse.UE4.Objects.Core.Misc;
using CUE4Parse.UE4.Versions;
using Newtonsoft.Json;
using Serilog;

const string ContentPrefix = "DungeonCrawler/Content/DungeonCrawler/";

if (args.Length < 3) { Console.Error.WriteLine("see Program.cs header for usage"); return 2; }
var mode = args[0];
var paks = args[1];
var game = Enum.Parse<EGame>("GAME_" + args[2]);
var verbose = Environment.GetEnvironmentVariable("DAD_VERBOSE") == "1";

Log.Logger = new LoggerConfiguration()
    .MinimumLevel.Is(verbose ? Serilog.Events.LogEventLevel.Warning : Serilog.Events.LogEventLevel.Fatal)
    .WriteTo.Console(standardErrorFromLevel: Serilog.Events.LogEventLevel.Verbose)
    .CreateLogger();
CUE4Parse.Globals.LogVfsMounts = false;
CUE4Parse.CUE4ParseLog.UseLogger(Log.Logger);

var oodle = Path.Combine(AppContext.BaseDirectory, OodleHelper.OodleFileName);
if (!File.Exists(oodle)) OodleHelper.DownloadOodleDll(ref oodle!);
OodleHelper.Initialize(oodle);

var provider = new DefaultFileProvider(paks, SearchOption.TopDirectoryOnly, true, new VersionContainer(game));
if (mode is "export" or "probe" or "icons" or "artmap") provider.MappingsContainer = new FileUsmapTypeMappingsProvider(args[3]);
provider.Initialize();
// The IoStore containers are not encrypted; the legacy .pak index is (key not needed for data).
var aes = Environment.GetEnvironmentVariable("DAD_AES");
provider.SubmitKey(new FGuid(), new FAesKey(string.IsNullOrEmpty(aes) ? "0x" + new string('0', 64) : aes));
provider.PostMount();
Console.Error.WriteLine($"Mounted {provider.MountedVfs.Count} containers, {provider.Files.Count} files");

var json = new JsonSerializerSettings { Formatting = Formatting.Indented };

switch (mode)
{
    case "list":
    {
        var lines = provider.Files.Values.Select(f => $"{f.Path}\t{f.Size}").OrderBy(s => s, StringComparer.Ordinal);
        File.WriteAllLines(args[3], lines);
        Console.Error.WriteLine($"Listed {provider.Files.Count} files");
        return 0;
    }
    case "raw":
    {
        var data = provider.SaveAsset(args[3]);
        File.WriteAllBytes(args[4], data);
        return 0;
    }
    case "probe":
    {
        Environment.SetEnvironmentVariable("DAD_VERBOSE", "1");
        Console.WriteLine(JsonConvert.SerializeObject(provider.LoadPackage(args[4]).GetExports(), json));
        return 0;
    }
    case "export":
    {
        var outDir = args[4];
        var prefixes = args.Skip(5).Select(p => p.ToLowerInvariant()).ToArray();
        var files = provider.Files.Keys
            .Where(k => k.EndsWith(".uasset") && prefixes.Any(p => k.ToLowerInvariant().StartsWith(p)))
            .OrderBy(k => k).ToList();
        Console.Error.WriteLine($"Exporting {files.Count} assets");
        var failures = new ConcurrentBag<string>();
        int ok = 0;
        Parallel.ForEach(files, new ParallelOptions { MaxDegreeOfParallelism = Environment.ProcessorCount }, f =>
        {
            try
            {
                var text = JsonConvert.SerializeObject(provider.LoadPackage(f).GetExports(), json);
                var rel = f.StartsWith(ContentPrefix, StringComparison.OrdinalIgnoreCase) ? f[ContentPrefix.Length..] : f;
                var dst = Path.Combine(outDir, rel[..^".uasset".Length] + ".json");
                Directory.CreateDirectory(Path.GetDirectoryName(dst)!);
                File.WriteAllText(dst, text);
                Interlocked.Increment(ref ok);
            }
            catch (Exception e) { failures.Add($"{f}\t{e.GetType().Name}: {e.Message.Split('\n')[0]}"); }
        });
        File.WriteAllLines(Path.Combine(outDir, "_export_failures.tsv"), failures.OrderBy(s => s));
        Console.Error.WriteLine($"Done: {ok} ok, {failures.Count} failed");
        return 0;
    }
    case "artmap":
    {
        // The item V2 asset's art reference is not decodable with the current usmap, but the import
        // table still names it: /Game/DungeonCrawler/Item/.../<Art>. Read it from the raw package.
        var rx = new System.Text.RegularExpressions.Regex(@"/Game/DungeonCrawler/Item/[A-Za-z0-9_/]+");
        var items = provider.Files.Keys
            .Where(k => k.EndsWith(".uasset") && k.Contains("/Data/Generated/V2/Item/Item/", StringComparison.OrdinalIgnoreCase))
            .Distinct().OrderBy(k => k).ToList();
        var rows = new ConcurrentBag<string>();
        var artCache = new ConcurrentDictionary<string, string>();
        Parallel.ForEach(items, f =>
        {
            var name = Path.GetFileNameWithoutExtension(f);
            try
            {
                var bytes = provider.SaveAsset(f);
                var text = System.Text.Encoding.ASCII.GetString(bytes);
                var arts = rx.Matches(text).Select(m => m.Value)
                    .SelectMany(v => SplitConcatenated(v)).Where(v => v.StartsWith("/Game/DungeonCrawler/Item/") && !v.Contains("_Sound")).Distinct().ToList();
                foreach (var art0 in arts)
                {
                    // the name table may glue the next name on ("…/LongswordId.Item…"): trim until the package exists
                    var art = art0;
                    string Pkg(string a) => a.Replace("/Game/", "DungeonCrawler/Content/") + ".uasset";
                    // FNames keep "_5001" as a separate number, so the per-rarity art package is "<art>_<rarity>"
                    var rarity = name.Length > 5 && char.IsDigit(name[^1]) ? name[^4..] : "";
                    bool Has(string a) => provider.Files.ContainsKey(Pkg(a));
                    while (art.Length > 30 && !Has(art) && !Has(art + "_" + rarity)) art = art[..^1];
                    if (rarity != "" && Has(art + "_" + rarity)) art = art + "_" + rarity;
                    else if (!Has(art)) continue;
                    var icons = artCache.GetOrAdd(art, a =>
                    {
                        try
                        {
                            var t = System.Text.Encoding.ASCII.GetString(provider.SaveAsset(Pkg(a)));
                            return string.Join(",", System.Text.RegularExpressions.Regex.Matches(t, @"Item_Icon_[A-Za-z0-9_]+")
                                .Select(m => m.Value)
                                .Distinct());
                        }
                        catch (Exception e) { return "error:" + e.GetType().Name; }
                    });
                    rows.Add($"{name}\t{art}\t{icons}");
                }                if (arts.Count == 0) rows.Add($"{name}\t\t");
            }
            catch (Exception e) { rows.Add($"{name}\terror\t{e.GetType().Name}\t"); }
        });
        File.WriteAllLines(args[4], rows.OrderBy(s => s));
        Console.Error.WriteLine($"artmap: {items.Count} items, {rows.Count} rows");
        return 0;
    }
    case "icons":
    {
        var outDir = args[4];
        var maxSize = args.Length > 5 ? int.Parse(args[5]) : 256;
        var fmt = args.Length > 6 ? args[6] : "webp";
        var filter = args.Length > 7 ? args[7] : "";
        Directory.CreateDirectory(outDir);
        var files = provider.Files.Keys
            // a filter containing '/' selects every texture under that path (e.g. "UI/Resources/IconSpell/")
            .Where(k => k.EndsWith(".uasset") && (filter.Contains('/')
                        ? k.Contains(filter, StringComparison.OrdinalIgnoreCase)
                        : Path.GetFileName(k).StartsWith("Item_Icon_", StringComparison.OrdinalIgnoreCase)
                          && (filter == "" || Path.GetFileName(k).Contains(filter, StringComparison.OrdinalIgnoreCase))))
            .Distinct().OrderBy(k => k).ToList();
        Console.Error.WriteLine($"Exporting {files.Count} icons");
        var failures = new ConcurrentBag<string>();
        var index = new ConcurrentBag<string>();
        int ok = 0, skipped = 0;
        var opts = new ExportOptions();
        Parallel.ForEach(files, new ParallelOptions { MaxDegreeOfParallelism = Environment.ProcessorCount }, f =>
        {
            var name = Path.GetFileNameWithoutExtension(f);
            var dst = Path.Combine(outDir, name + "." + fmt);
            var rel = f.StartsWith(ContentPrefix, StringComparison.OrdinalIgnoreCase) ? f[ContentPrefix.Length..] : f;
            index.Add($"{name}\t{rel}");
            if (File.Exists(dst)) { Interlocked.Increment(ref skipped); return; }
            try
            {
                var tex = provider.LoadPackage(f).GetExports().OfType<UTexture2D>().FirstOrDefault()
                          ?? throw new Exception("no UTexture2D export");
                var decoded = tex.Decode(maxSize, opts) ?? throw new Exception("decode returned null");
                using var bmp = decoded.ToSkBitmap();
                using var img = SKImage.FromBitmap(bmp);
                using var data = fmt == "png" ? img.Encode(SKEncodedImageFormat.Png, 100) : img.Encode(SKEncodedImageFormat.Webp, 90);
                File.WriteAllBytes(dst, data.ToArray());
                Interlocked.Increment(ref ok);
            }
            catch (Exception e) { failures.Add($"{f}\t{e.GetType().Name}: {e.Message.Split('\n')[0]}"); }
        });
        File.WriteAllLines(Path.Combine(outDir, "_index.tsv"), index.OrderBy(s => s));
        File.WriteAllLines(Path.Combine(outDir, "_failures.tsv"), failures.OrderBy(s => s));
        Console.Error.WriteLine($"Done: {ok} written, {skipped} already there, {failures.Count} failed");
        return 0;
    }
    default:
        Console.Error.WriteLine($"unknown mode {mode}");
        return 2;
}


// Names in the raw name table run together ("/Game/A/B/Game/C/D"); split them back apart.
static IEnumerable<string> SplitConcatenated(string s)
{
    var parts = s.Split("/Game/", StringSplitOptions.RemoveEmptyEntries);
    foreach (var p in parts) yield return "/Game/" + p;
}