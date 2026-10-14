using OpenCad.Api;

namespace OpenCad.Api.Samples;

/// <summary>
/// Sample interactive command: two clicks commit a line. Not registered with the host.
/// </summary>
public sealed class TwoPointLineCommand : IInteractiveCommand
{
    private Point3d? _start;

    public string Prompt => _start is null ? "Specify first point" : "Specify next point";

    public CommandResult OnPoint(Point3d point)
    {
        if (_start is not Point3d start)
        {
            _start = point;
            return CommandResult.NeedPoint();
        }

        return CommandResult.Commit(new LineSpec(start, point), end: true);
    }

    public CommandResult OnEnter() => CommandResult.Cancel();

    public IReadOnlyList<PreviewCurve> OnCursorMove(Point3d point)
    {
        if (_start is not Point3d start)
            return Array.Empty<PreviewCurve>();
        return new PreviewCurve[] { new PreviewPolyline(new[] { start, point }) };
    }
}
