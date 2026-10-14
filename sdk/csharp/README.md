# OpenCad.Api

C# 开发接口，给要在 OpenCADStudio 里写交互命令的人引用。目标框架 `net8.0`。

宿主现在还不会加载这个程序集。编辑器只认 Rust `cdylib` 里的 `BuiltinPlugin`。这份库是那一层外面的 C# 契约：以后由一个 Rust 壳把 `IPlugin` / `IInteractiveCommand` 转成宿主已经有的 `HostApi` 和 `InteractiveCommand`。

## 开发者实现什么

- `IPlugin`：插件身份，以及一条命令进来时的 `Execute`。
- `IHost`：能对当前图做的事。增删实体、选择、命令行、CLAYER / SNAPANG、只读的 CTAB、字符串 XDATA、开始一条交互命令。
- `IInteractiveCommand`：提示、点、回车、选实体、鼠标移动时的预览。
- `CommandResult`：还要点、提交并继续、提交并结束、结束、取消。
- `EntitySpec`：第一批只有直线、圆、圆弧、文字。图层留空就用当前层。

`Samples/TwoPointLineCommand.cs` 是点两次画一条线的写法，没有注册到宿主。

## 和宿主的对应

| C# | 宿主 |
| --- | --- |
| `IInteractiveCommand` | `InteractiveCommand` |
| `CommandResult` | `CommandStep` |
| `PreviewCurve` | `PreviewWire` |
| `IHost.Add` / `Remove` | `add_entity` / `remove_entity` |
| `Execute` | `execute_command` |
| `CurrentLayer` / `SnapAngleDegrees` / `CurrentLayout` | `CLAYER` / `SNAPANG` / `CTAB`（布局只读） |

没有 `document_mut`。进程外改快照不会写回图，所以接口里不提供。

## 还没做的

Rust 壳（把这个程序集装进插件进程，或跟一个单独的 C# 进程讲同一套对象）还没有。在它接上之前，引用本库只能编译自己的命令，不能在编辑器里跑。
