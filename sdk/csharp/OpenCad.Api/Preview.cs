namespace OpenCad.Api;

/// <summary>
/// Rubber-band geometry drawn while an interactive command is active.
/// Matches the host's <c>PreviewWire</c>: polyline, circle, or arc.
/// Color is RGBA, each channel 0..1. Null uses the host cyan.
/// </summary>
public abstract record PreviewCurve
{
    public float[]? Color { get; init; }
}

public sealed record PreviewPolyline(IReadOnlyList<Point3d> Points) : PreviewCurve;

public sealed record PreviewCircle(Point3d Center, double Radius) : PreviewCurve;

public sealed record PreviewArc(Point3d Center, double Radius, double StartAngleRad, double EndAngleRad) : PreviewCurve;
