# P&ID 界面用的两种字体

设计稿 `docs/pid-platform/design/pid-platform.pen` 有两个字体变量：`font-ui` 是 Inter，
`font-mono` 是 JetBrains Mono。Windows 不带这两种字体，没有它们时 P&ID 界面的文字落到回退字体上，
字宽就不是设计稿的字宽。这个目录放的是设计稿用到的那几个字重，原样取自各自的官方发布包。

程序把这六个文件编进可执行文件（`src/ui/style/pid_fonts.rs`），在 P&ID 模块打开时装进界面的字体库。
一直留在通用界面的会话（`--classic`）不装，它画出来的东西不会因此改变。图纸里的文字用的是另一个
字体库（`src/scene/text/ttf_glyph.rs`），不受影响。

## 文件

| 文件 | 字体族 | 字重 | 发布包里的路径 | 字节数 | SHA-256 |
|---|---|---|---|---|---|
| `Inter-Regular.ttf` | Inter | 400 | `extras/ttf/Inter-Regular.ttf` | 411640 | `40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82` |
| `Inter-Medium.ttf` | Inter | 500 | `extras/ttf/Inter-Medium.ttf` | 417300 | `97ad806f526e41546d46365bb3a393145f75b7b1568913db74549ad8b8dba872` |
| `Inter-SemiBold.ttf` | Inter | 600 | `extras/ttf/Inter-SemiBold.ttf` | 419744 | `78a843fade9d4612a5567302fb595b56976eb5fcebf4fea5a5912d638bafcde3` |
| `JetBrainsMono-Regular.ttf` | JetBrains Mono | 400 | `fonts/ttf/JetBrainsMono-Regular.ttf` | 273900 | `a0bf60ef0f83c5ed4d7a75d45838548b1f6873372dfac88f71804491898d138f` |
| `JetBrainsMono-SemiBold.ttf` | JetBrains Mono | 600 | `fonts/ttf/JetBrainsMono-SemiBold.ttf` | 277092 | `1b3bfa1ed5665a4ce3f9feb68d2d4e40e70bf8b4b7d9a3edd418f321b4e166a0` |
| `JetBrainsMono-Bold.ttf` | JetBrains Mono | 700 | `fonts/ttf/JetBrainsMono-Bold.ttf` | 277828 | `5590990c82e097397517f275f430af4546e1c45cff408bde4255dad142479dcb` |
| `Inter-LICENSE.txt` | Inter 的许可文本 | | `LICENSE.txt` | 4380 | `262481e844521b326f5ecd053e59b98c8b2da78c8ee1bdbb6e8174305e54935a` |
| `JetBrainsMono-OFL.txt` | JetBrains Mono 的许可文本 | | `OFL.txt` | 4399 | `30f0c136e3c88e422d0791acd97238870f9054a9729bc34cf2ff0d4ed8cac4ad` |
| `JetBrainsMono-AUTHORS.txt` | JetBrains Mono 的作者名单 | | `AUTHORS.txt` | 350 | `33f99c94ef0a0909d7584a5d4bde1fb9b1630daa620f5a788c28c43327e79412` |

字重为什么是这几个：设计稿里 `$font-ui` 的文字用到 normal（400）、500、600，`$font-mono` 的文字用到
normal（400）、600、700，没有斜体。`check.cmd pid_fonts` 里有一条用例另读一遍设计稿来对这张清单，
设计稿多用了一个字重它就失败。

## 来源

- **Inter 4.1**：<https://github.com/rsms/inter/releases/tag/v4.1>（2024-11-16 发布）。
  发布包 <https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip>，33707794 字节，
  SHA-256 `9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e`。
  字体文件里自己写的版本是 `Version 4.001;git-9221beed3`。
- **JetBrains Mono 2.304**：<https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304>（2023-01-14 发布）。
  发布包 <https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip>，
  5622857 字节，SHA-256 `6f6376c6ed2960ea8a963cd7387ec9d76e3f629125bc33d1fdcd7eb7012f7bbf`。
  字体文件里自己写的版本是 `Version 2.304; ttfautohint (v1.8.4.7-5d5b)`。

2026-11-12 经 HTTPS 从 GitHub 的发布页取得。九个文件都是发布包里的原样字节：字体没有改、没有裁剪字符集、
没有改字体名；只有三个文本文件为了不重名，文件名前面加了字体的名字。

## 许可

两种字体都按 SIL Open Font License 1.1 发布，许可文本就是上表里的两个文件，随字体一起分发
（许可第 2 条的要求）。字体没有修改，所以不涉及保留字体名的条款。

## 换版本时

换掉文件，改上面的表（路径、字节数、SHA-256、来源一节的版本与地址）。`check.cmd pid_fonts` 里有一条用例
拿表里的 SHA-256 去对编进程序的字节，另一条对这个目录里有哪些 `.ttf`，漏改会失败。
