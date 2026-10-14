namespace OpenCad.Api;

/// <summary>
/// What a C# plugin may ask the running editor to do.
/// Maps onto <c>HostApi</c> methods the desktop host actually implements.
/// <see cref="DocumentPath"/> and <see cref="CurrentLayout"/> are read-only.
/// Direct mutation of a document snapshot is intentionally absent: writes go
/// through <see cref="Add"/>, <see cref="Remove"/>, and <see cref="Execute"/>.
/// </summary>
public interface IHost
{
    ulong TabId { get; }

    /// <summary>Path of the drawing in this tab, if it has one.</summary>
    string? DocumentPath { get; }

    IReadOnlyList<EntityId> Selection { get; }

    void SetSelection(IReadOnlyList<EntityId> ids);

    /// <summary>CLAYER. Null when the host has no current layer.</summary>
    string? CurrentLayer { get; set; }

    /// <summary>SNAPANG, degrees.</summary>
    double SnapAngleDegrees { get; set; }

    /// <summary>CTAB. The host does not accept writes.</summary>
    string? CurrentLayout { get; }

    void Info(string message);

    void Output(string message);

    void Error(string message);

    /// <summary>Open an undo scope. Later adds in this command join it.</summary>
    void PushUndo(string label);

    EntityId Add(EntitySpec entity);

    bool Remove(EntityId id);

    /// <summary>
    /// Run a command-line string the way the user would type it
    /// (<c>HostApi::execute_command</c>).
    /// </summary>
    bool Execute(string commandLine);

    /// <summary>
    /// String pairs stored as XDATA under <paramref name="appName"/>.
    /// Null when that application has no record on the entity.
    /// </summary>
    IReadOnlyDictionary<string, string>? ReadRecords(EntityId id, string appName);

    bool WriteRecords(EntityId id, string appName, IReadOnlyDictionary<string, string> pairs);

    bool RemoveRecords(EntityId id, string appName);

    void StartInteractive(IInteractiveCommand command);
}
