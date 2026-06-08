using CUE4Parse.Compression;
using CUE4Parse.FileProvider;
using CUE4Parse.MappingsProvider;
using CUE4Parse.UE4.Assets.Exports;
using CUE4Parse.UE4.Assets.Exports.Actor;
using CUE4Parse.UE4.Objects.Core.Math;
using CUE4Parse.UE4.Objects.Engine;
using CUE4Parse.UE4.Objects.UObject;
using CUE4Parse.UE4.Versions;

const string rootDirectory = @"D:\SteamLibrary\steamapps\common\Satisfactory\";
const string directory = rootDirectory + @"FactoryGame\Content\Paks\";
const string levelPath =
    "FactoryGame/Content/FactoryGame/Map/GameLevel01/Persistent_Level.umap.PersistentLevel";

await OodleHelper.InitializeAsync(OodleHelper.OodleFileName);
Console.Error.WriteLine(OodleHelper.Instance is null ? "Oodle: NOT loaded" : "Oodle: loaded");

var provider = new DefaultFileProvider(
    directory,
    SearchOption.AllDirectories,
    new VersionContainer(EGame.GAME_UE5_6),
    StringComparer.Ordinal);
provider.Initialize();
provider.Mount();

var levelCandidates = provider.Files.Keys
    .Where(k => k.Contains("GameLevel01", StringComparison.OrdinalIgnoreCase) && k.EndsWith(".umap", StringComparison.OrdinalIgnoreCase))
    .Take(30)
    .ToList();

Console.Error.WriteLine($"Mounted files: {provider.Files.Count}");
Console.Error.WriteLine("Level candidates:");
foreach (var c in levelCandidates)
    Console.Error.WriteLine($"  {c}");

var level = provider.LoadPackageObject<ULevel>(levelPath);

using var writer = new StreamWriter("extracted-resources.json");
writer.WriteLine("[");

foreach (var node in level.Actors.Select(a => a.Load()).Where(a => a is { ExportType: "BP_ResourceNode_C" }))
{
    if (node is null) continue;

    var name = node.Name;
    var location = node.Get<FPackageIndex>("mBoxComponent").Load().Get<FVector>("RelativeLocation");
    var resource = node.Get<FPackageIndex>("mResourceClass").Name;
    var purity = node.GetOrDefault<FName>("mPurity", "RP_Normal").ToString();

    writer.WriteLine(
        $"""["{node.ExportType}", "{name}", [{location.X}, {location.Y}, {location.Z}], "{resource}", "{purity}"],""");
}

foreach (var node in level.Actors.Select(a => a.Load()).Where(a => a is { ExportType: "BP_ResourceNodeGeyser_C" }))
{
    if (node is null) continue;

    var name = node.Name;
    var location = node.Get<FPackageIndex>("mBoxComponent").Load().Get<FVector>("RelativeLocation");
    var purity = node.GetOrDefault<FName>("mPurity", "RP_Normal").ToString();

    writer.WriteLine(
        $"""["{node.ExportType}", "{name}", [{location.X}, {location.Y}, {location.Z}], "{purity}"],""");
}

foreach (var node in level.Actors.Select(a => a.Load()).Where(a => a is { ExportType: "BP_FrackingCore_C" }))
{
    if (node is null) continue;

    var name = node.Name;
    var location = node.Get<FPackageIndex>("mBoxComponent").Load().Get<FVector>("RelativeLocation");

    writer.WriteLine(
        $"""["{node.ExportType}", "{name}", [{location.X}, {location.Y}, {location.Z}]],""");
}

foreach (var node in level.Actors.Select(a => a.Load()).Where(a => a is { ExportType: "BP_FrackingSatellite_C" }))
{
    if (node is null) continue;

    var name = node.Name;
    var location = node.Get<FPackageIndex>("mBoxComponent").Load().Get<FVector>("RelativeLocation");
    var resource = node.Get<FPackageIndex>("mResourceClass").Name;
    var purity = node.GetOrDefault<FName>("mPurity", "RP_Normal").ToString();
    var core = node.Get<FPackageIndex>("mCore").Name;

    writer.WriteLine(
        $"""["{node.ExportType}", "{name}", [{location.X}, {location.Y}, {location.Z}], "{resource}", "{purity}", "{core}"],""");
}

writer.WriteLine("]");
