namespace OpenCad.Api;

/// <summary>
/// A click-driven command. The host shows <see cref="Prompt"/>, then forwards
/// points, Enter, entity picks, and cursor moves. This is the C# face of
/// <c>InteractiveCommand</c>.
/// </summary>
public interface IInteractiveCommand
{
    string Prompt { get; }

    /// <summary>
    /// When true, the next click selects an existing entity and the host calls
    /// <see cref="OnObjectPick"/> instead of <see cref="OnPoint"/>.
    /// </summary>
    bool NeedsObjectPick => false;

    CommandResult OnPoint(Point3d point);

    CommandResult OnEnter() => CommandResult.Cancel();

    CommandResult OnObjectPick(EntityId id, Point3d point) => CommandResult.Cancel();

    IReadOnlyList<PreviewCurve> OnCursorMove(Point3d point) => Array.Empty<PreviewCurve>();
}
