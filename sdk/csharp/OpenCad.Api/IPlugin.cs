namespace OpenCad.Api;

public sealed record PluginInfo(
    string Id,
    string Name,
    string Version,
    string Description,
    IReadOnlyList<string> Commands);

/// <summary>
/// Entry a C# assembly exposes. A future Rust shell discovers one implementation,
/// calls <see cref="OnLoad"/> once, and <see cref="Execute"/> for each command.
/// </summary>
public interface IPlugin
{
    PluginInfo Info { get; }

    void OnLoad(IHost host) { }

    /// <summary>Return true when <paramref name="command"/> belongs to this plugin.</summary>
    bool Execute(IHost host, string command);
}
