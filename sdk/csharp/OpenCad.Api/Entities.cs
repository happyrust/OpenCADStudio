namespace OpenCad.Api;

/// <summary>
/// Something the host can commit. The Rust bridge turns these into
/// <c>opencadcodec</c> entities. This set is the first cut: line, circle, arc, text.
/// </summary>
public abstract record EntitySpec
{
    /// <summary>Layer name. Null keeps the host's current layer (CLAYER).</summary>
    public string? Layer { get; init; }
}

public sealed record LineSpec(Point3d Start, Point3d End) : EntitySpec;

public sealed record CircleSpec(Point3d Center, double Radius) : EntitySpec;

/// <summary>Counter-clockwise arc, angles in radians.</summary>
public sealed record ArcSpec(Point3d Center, double Radius, double StartAngleRad, double EndAngleRad) : EntitySpec;

public sealed record TextSpec(Point3d Position, string Value, double Height) : EntitySpec;
