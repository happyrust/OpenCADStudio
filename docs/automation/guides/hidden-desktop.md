# 不动鼠标、不抢焦点地看真实窗口

要看的是真实的编辑器窗口（GPU 渲染、真实的字体和布局），而这台机器上有人正在工作。办法是把编辑器开在 Windows 的另一个**桌面**上：别的桌面上的窗口不会出现在人面前，抢不到前台，也碰不到指针。命令和截图都走编辑器自带的回环 REST 通道。

| 文件 | 用途 |
|---|---|
| `docs/automation/hidden_desktop.py` | 库：`HiddenEditor`（起实例、发命令、截图、关闭）、`desktop_state` / `same_desktop`（确认桌面没被碰过）、`start_drawing` |
| `docs/automation/hidden_steps.py` | 不写代码的用法：按一份 JSON 步骤驱动一个或几个实例 |
| `docs/automation/hidden_steps_example.json` | 两个实例的例子，任何检出上都能跑 |

只要 Windows 和 Python 3 的标准库；装了 Pillow 才能把截图裁到某个控件。

## 起实例

```powershell
python docs\automation\hidden_steps.py target\debug\OpenCADStudio.exe target\ui-audit\run docs\automation\hidden_steps_example.json --instances 2 --backend vulkan
```

或者在代码里：

```python
from hidden_desktop import HiddenEditor, desktop_state, same_desktop, start_drawing

out = "target/ui-audit/run"
before = desktop_state()
with HiddenEditor("target/debug/OpenCADStudio.exe", 8231, out, [start_drawing(out), "--backend", "vulkan"]) as editor:
    editor.wait_ready()      # REST 通道应答
    editor.find_window()     # 主窗口出现；之后才能关窗口、换算截图的缩放
    editor.command("LINE 0,0 100,50")
    editor.capture("line")
print(same_desktop(before, desktop_state()))
```

要点：

- **必须给一张图纸。** `--http` 不带图纸是没有窗口的服务模式；给一个不存在的路径，编辑器永远不会就绪。没有现成图纸时用 `start_drawing(目录)`（`hidden_steps.py` 不带 `--open` 时就是它）：把仓库里的一张小夹具复制过去，编辑器不会在夹具旁边写任何东西。
- **设置是隔离的。** 编辑器的 `%APPDATA%` 指到 `<输出目录>\appdata`，读不到也写不到本人的设置、最近文件和会话清单。界面语言因此跟随系统（新设置里 `language` 是 `system`）。
- **`--backend vulkan`**：这台机器上 DX12 后端的模型视口不出图（方案文档 D7，原因未查）。只看面板可以不加。
- 其余命令行参数原样写进 `args`（`hidden_steps.py` 用 `--editor-args "..."`），环境变量用 `env=`（`--env 名=值`），`%TEMP%` 用 `temp=`（`--temp`）。
- 每个实例一个随机的 `OCS_API_TOKEN`，只在本机回环地址上监听。

## 发命令

`editor.command("...")` 就是在命令行里敲一行（`POST /api/v1/commands {"cmd": ...}`），在活动图纸上执行。`editor.op(名字, 参数)` 是 REST 接口的任意一个操作，清单见 `docs/automation/API-SPEC.md`。命令行说了什么用 `history` 读回，图纸的修改标记在 `editor.documents()` 里。

`hidden_steps.py` 的步骤：

```json
[
  ["cmd", "PIDBACKUPOPEN D:\\backup"],
  ["wait", 1.5],
  ["select", "备份文件 > Export.dmp > SQPlantpid"],
  ["capture", "whole-window"],
  ["crop", "panel", "pid-backup-panel"],
  ["state", "走完之后"],
  ["history", 5]
]
```

完整的步骤种类写在 `hidden_steps.py` 开头。隐藏桌面上没有指针可点，所以每一步都得是命令做得到的事；本分支为此给备份面板加了 `PIDBACKUPOPEN Select 标签 > 标签 > …`，按树里显示的标签逐级展开并选中，效果与逐级点击相同。

## 截图

`editor.capture(名字)` 让编辑器把自己画的整个窗口写成 `<输出目录>\<序号>-<名字>.png`；`scope="viewport"` 只要绘图区。这是编辑器渲染出来的像素，不是屏幕抓图，所以窗口不在屏幕上也照样截得到，内容与屏幕上看到的相同。

`editor.crop(名字, 控件)` 截完再裁到一个控件。能裁的是向 `ui_bounds` 报告位置的控件，例如 `pid-backup-panel`；`editor.bounds(前缀)` 列出它们。位置是逻辑像素，截图是物理像素，`find_window()` 取到的缩放用来换算。

每个实例的输出目录里还有 `editor-stderr.log`（编辑器自己的输出）和 `report.txt`（脚本说过的每一行）。

## 确认指针和前台窗口没变

开始前、结束后各取一次 `desktop_state()`，`same_desktop(之前, 之后)` 给一句结论：

```text
desktop untouched: pointer still at (122, 553), foreground still 0x5f1072 "Cursor Agents"
```

`hidden_steps.py` 自己做这件事，结论是它的最后一行。指针或前台窗口变了的时候，它再看这段时间有没有真实的键盘鼠标输入：有，是人在用机器；没有，就是这次运行造成的，脚本以退出码 2 结束。

## 收尾

`with` 块结束（或 `editor.close()`）时：向主窗口投递关闭消息，等 6 秒；没关掉就结束进程。图纸被命令改动过的时候编辑器会问要不要保存，没有人回答，所以这种情况一定走到「结束进程」，退出码 1，属于预期：

```text
editor pid 3680 did not close (an unsaved drawing asks to be saved) and was ended, exit code 1
```

隐藏桌面上的窗口没有人看得见，漏掉一个进程也就没有人会去关。所以编辑器进程挂在一个作业对象上：脚本以任何方式结束（包括被杀掉），编辑器和它起的子进程一起结束。启动时那一行里的 `ends with this script: True` 说的就是这个。事后可以再确认一遍：

```powershell
Get-CimInstance Win32_Process -Filter "Name='OpenCADStudio.exe'" | Select-Object ProcessId, CommandLine
```

## 同时起几个实例

可以，实测两个实例并排运行互不影响（各约 2 秒就绪，各自画图、各自截图）。每个实例要有自己的端口；桌面、设置目录和输出目录跟着端口各有一份。带 `--http` 的进程不会把图纸转交给已经开着的编辑器，所以不需要 `--new-instance`。

`hidden_steps.py --instances N` 用端口 `--port`、`--port`+1、…，输出在 `<输出目录>\0`、`\1`、…。以数字开头的步骤发给那一个实例，其余发给 0 号：

```json
[
  [0, "cmd", "LINE 0,0 100,50"],
  [1, "cmd", "CIRCLE 50,25 20"],
  [0, "capture", "line"],
  [1, "capture", "circle"]
]
```

各实例的命令行不一样的时候（协同会话的两个客户端用不同的 `--user`）：

```powershell
python docs\automation\hidden_steps.py target\debug\OpenCADStudio.exe target\ui-audit\collab steps.json --instances 2 `
    --editor-args "--collab ws://127.0.0.1:9000 --drawing D07" --editor-args-0 "--user alice" --editor-args-1 "--user bob"
```

每多一个实例多占一份显存和内存：debug 构建的一个实例实测提交内存约 470 MB、工作集约 220 MB。

## 做不到的

- **点击、拖动、滚轮、悬停。** 隐藏桌面上没有指针；往窗口投递鼠标消息也到不了控件（试过）。点击命中靠 `iced_test` 里按画出来的位置点的用例，真实窗口只验「命令做完之后画成什么样」。
- **系统的文件对话框。** 它会开在隐藏桌面上，没有人能点。用带路径的命令（`PIDBACKUPOPEN <路径>`、`OPEN <路径>`），或者先 `FILEDIA 0`。
- 首次启动的「默认应用程序」提示是一个单独的小窗口，留在隐藏桌面上，不挡命令，也不在主窗口的截图里。
- 只有 Windows。
