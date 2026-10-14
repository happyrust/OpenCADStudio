namespace OpenCad.Api;

/// <summary>
/// What an interactive step tells the host. Same outcomes as <c>CommandStep</c>:
/// keep asking, commit and keep asking, commit and finish, finish, cancel.
/// </summary>
public abstract record CommandResult
{
    public static CommandResult NeedPoint() => NeedPointResult.Instance;

    public static CommandResult Done() => DoneResult.Instance;

    public static CommandResult Cancel() => CancelResult.Instance;

    public static CommandResult Commit(EntitySpec entity, bool end = false) =>
        new CommitResult(new[] { entity }, end);

    public static CommandResult CommitMany(IReadOnlyList<EntitySpec> entities, bool end = false) =>
        new CommitResult(entities, end);

    private sealed record NeedPointResult : CommandResult
    {
        public static readonly NeedPointResult Instance = new();
    }

    private sealed record DoneResult : CommandResult
    {
        public static readonly DoneResult Instance = new();
    }

    private sealed record CancelResult : CommandResult
    {
        public static readonly CancelResult Instance = new();
    }

    public sealed record CommitResult(IReadOnlyList<EntitySpec> Entities, bool End) : CommandResult;
}
