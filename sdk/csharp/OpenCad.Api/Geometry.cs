namespace OpenCad.Api;

/// <summary>World point. Z is 0 for planar input.</summary>
public readonly record struct Point3d(double X, double Y, double Z)
{
    public static Point3d Origin => new(0, 0, 0);
}

/// <summary>Entity handle as the host stores it. Zero is not a valid entity.</summary>
public readonly record struct EntityId(ulong Value)
{
    public bool IsValid => Value != 0;
}
