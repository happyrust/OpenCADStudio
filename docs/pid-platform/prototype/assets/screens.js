/* Consolidated screen bundle for the P&ID platform prototype.
   Classic script, ES5, no modules/network. This file concatenates, in load
   order, the former screens-shell.js (registry + test surface), screens-data.js
   (seed dataset) and screens-workbench.js (workbench & drawings renderers),
   each preserved as its own IIFE. */

/* Screen registry, placeholder renderers and the programmatic test surface
   for the P&ID platform prototype. Classic script, ES5, no modules/network. */
(function () {
  "use strict";

  var ROUTE_PREFIX = "#/screen/";

  var SCREENS = [
    {
      id: "workbench",
      title: "工作台",
      group: "总览",
      desc: "项目入口与待办汇总页，呈现项目概览、最近打开的图纸与任务提醒。",
      points: ["项目概览与最近图纸", "待办事项与消息提醒", "常用功能快捷入口"]
    },
    {
      id: "drawings",
      title: "图纸管理",
      group: "总览",
      desc: "按 PBS 层级组织图纸台账，维护图纸版本快照与发布状态。",
      points: ["PBS 层级浏览", "图纸清单与状态", "图纸版本快照管理"]
    },
    {
      id: "editor",
      title: "P&ID 编辑器",
      group: "绘制",
      desc: "图面绘制主界面，放置设备、管线、管路与仪表对象，经管口与连接点建立拓扑。",
      points: ["符号放置与移动", "管段绘制与连接点吸附", "设备管口与仪表回路关联"]
    },
    {
      id: "palette",
      title: "符号调色板",
      group: "绘制",
      desc: "按图例标准组织的符号选取面板，支持检索与拖拽进入画布。",
      points: ["符号分组浏览", "按图例标准过滤", "拖拽放置到画布"]
    },
    {
      id: "properties",
      title: "属性面板",
      group: "绘制",
      desc: "查看并编辑当前选中对象的位号与数据要素取值。",
      points: ["位号与属性编辑", "必选信息提示", "关联要素取值"]
    },
    {
      id: "typelib",
      title: "业务类型库",
      group: "库与规则",
      desc: "类型库的类与型树管理，维护属性模板并绑定数据要素。",
      points: ["类型树维护", "属性模板配置", "数据要素绑定"]
    },
    {
      id: "symbols",
      title: "标准图例定制",
      group: "库与规则",
      desc: "项目级图例标准的选定与符号启用管理入口。",
      points: ["图例标准选择", "符号启用与状态管理"]
    },
    {
      id: "symbol-editor",
      title: "符号编辑器",
      group: "库与规则",
      desc: "绘制与编辑符号图形，配置连接点与显示属性。",
      points: ["符号图形绘制", "连接点配置"]
    },
    {
      id: "tag-rules",
      title: "位号规则",
      group: "库与规则",
      desc: "定义各类对象的位号生成规则，提供位号预览与唯一校验。",
      points: ["位号规则定义", "位号预览", "唯一校验配置"]
    },
    {
      id: "color-rules",
      title: "颜色规则",
      group: "库与规则",
      desc: "按介质、管道等级等条件定义管线与图面对象的颜色规则。",
      points: ["规则条件定义", "颜色映射维护"]
    },
    {
      id: "masterdata",
      title: "主数据管理",
      group: "库与规则",
      desc: "维护项目主数据，包括公称直径序列、管道等级、绝热等级与流体介质。",
      points: ["公称直径序列", "管道等级", "流体介质"]
    },
    {
      id: "validation",
      title: "设计规则检查",
      group: "检查与交付",
      desc: "对图纸执行连通性与属性完整性的设计规则检查并呈现结果。",
      points: ["检查规则集", "检查结果列表", "定位到图面对象"]
    },
    {
      id: "bom",
      title: "材料清单",
      group: "检查与交付",
      desc: "按位号聚合管件、阀门与设备等对象实例，形成材料清单。",
      points: ["按位号聚合统计", "分类汇总", "清单导出"]
    },
    {
      id: "permissions",
      title: "权限管理",
      group: "系统",
      desc: "按角色配置功能入口与图纸范围的访问权限。",
      points: ["角色维护", "权限范围配置"]
    }
  ];

  var MAP = {};
  var i;
  for (i = 0; i < SCREENS.length; i++) {
    MAP[SCREENS[i].id] = SCREENS[i];
  }

  /* ---------- Programmatic test surface ---------- */

  var PROTO_KEYS = [
    "screen",
    "symbolCount",
    "selectedTag",
    "selectedProps",
    "violations",
    "bomRows",
    "paletteItemCount",
    "tagRulePreview"
  ];

  function protoSeed() {
    return {
      screen: "",
      symbolCount: 0,
      selectedTag: "",
      selectedProps: [],
      violations: [],
      bomRows: [],
      paletteItemCount: 0,
      tagRulePreview: ""
    };
  }

  var currentScreenId = "";

  /* One-shot merge staged by a screen renderer (see window.PidScreens below).
     renderScreenHtml runs before publishProto on every route pass, so a
     renderer can stage values here and have them applied to the freshly
     rebuilt window.__proto for the same screen. A merge staged for a
     different screen is dropped, so stale values never leak across routes. */
  var pendingProtoMerge = null;

  function setPendingProtoMerge(screenId, partial) {
    pendingProtoMerge = {
      screen: screenId,
      partial: partial && typeof partial === "object" ? partial : {}
    };
  }

  function publishProto(screenId) {
    currentScreenId = screenId;
    var proto = protoSeed();
    proto.screen = screenId;
    window.__proto = proto;
    var merge = pendingProtoMerge;
    pendingProtoMerge = null;
    if (merge && merge.screen === screenId) {
      window.__protoSet(merge.partial);
    }
    return proto;
  }

  /* Merge helper for later screens. Only the contractual keys are merged;
     `screen` stays owned by the router. Never triggers a re-render. */
  window.__protoSet = function (partial) {
    if (!partial || typeof partial !== "object") {
      return;
    }
    if (!window.__proto || typeof window.__proto !== "object") {
      publishProto(currentScreenId);
    }
    for (var j = 0; j < PROTO_KEYS.length; j++) {
      var key = PROTO_KEYS[j];
      if (key !== "screen" && Object.prototype.hasOwnProperty.call(partial, key)) {
        window.__proto[key] = partial[key];
      }
    }
    window.__proto.screen = currentScreenId;
  };

  /* ---------- Rendering helpers ---------- */

  function esc(text) {
    return String(text)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }

  function renderScreenHtml(id) {
    var def = MAP[id];
    if (!def) {
      return "";
    }
    /* Real screens register a renderer on window.PidScreens (registered by a
       later IIFE in this bundle). A renderer failure falls back to the
       placeholder so it never surfaces as an uncaught page error. */
    if (window.PidScreens && typeof window.PidScreens[id] === "function") {
      try {
        return window.PidScreens[id](def);
      } catch (err) {
        /* fall through to placeholder */
      }
    }
    var points = "";
    for (var k = 0; k < def.points.length; k++) {
      points += "<li>" + esc(def.points[k]) + "</li>";
    }
    return (
      '<section class="screen" data-screen="' + esc(id) + '">' +
      '<header class="screen__header">' +
      '<h1 class="screen__title" data-testid="screen-title">' + esc(def.title) + "</h1>" +
      '<p class="screen__desc">' + esc(def.desc) + "</p>" +
      "</header>" +
      '<div class="panel">' +
      '<p class="panel__lead">本界面已在规格包中完成定义，当前原型阶段仅提供外壳占位，' +
      "具体功能将随迭代逐步挂载。</p>" +
      '<ul class="spec-list">' + points + "</ul>" +
      '<p class="screen__meta">路由 ' + esc(ROUTE_PREFIX + id) +
      " · 分组 " + esc(def.group) + " · 界面 ID " + esc(id) + "</p>" +
      "</div>" +
      "</section>"
    );
  }

  function buildModuleNav(navEl) {
    var html = "";
    var openGroup = false;
    var lastGroup = null;
    for (var k = 0; k < SCREENS.length; k++) {
      var screen = SCREENS[k];
      if (screen.group !== lastGroup) {
        if (openGroup) {
          html += "</div>";
        }
        html += '<div class="module-nav__group">' +
          '<div class="module-nav__group-title">' + esc(screen.group) + "</div>";
        lastGroup = screen.group;
        openGroup = true;
      }
      html += '<a class="module-nav__item" data-testid="module-nav-item" data-screen="' +
        esc(screen.id) + '" href="' + esc(ROUTE_PREFIX + screen.id) + '">' +
        esc(screen.title) + "</a>";
    }
    if (openGroup) {
      html += "</div>";
    }
    navEl.innerHTML = html;
  }

  window.PidShell = {
    ROUTE_PREFIX: ROUTE_PREFIX,
    DEFAULT_SCREEN: "workbench",
    SCREENS: SCREENS,
    MAP: MAP,
    PROTO_KEYS: PROTO_KEYS,
    publishProto: publishProto,
    setPendingProtoMerge: setPendingProtoMerge,
    renderScreenHtml: renderScreenHtml,
    buildModuleNav: buildModuleNav,
    currentScreenId: function () {
      return currentScreenId;
    }
  };

  /* Publish the seeded surface immediately so window.__proto always exists. */
  publishProto("");
})();

/* Seed dataset for the P&ID platform prototype.
   Classic script, ES5, no modules/network. Exposes window.PID_DATA.
   Counts on `project` are derived from the arrays below so the workbench
   stats stay consistent when the data changes. */
(function () {
  "use strict";

  var pbs = [
    {
      id: "plant-01",
      name: "炼油厂区",
      level: "工厂",
      children: [
        {
          id: "unit-cdu",
          name: "常减压装置",
          level: "装置",
          children: [
            { id: "cell-heat", name: "原油换热单元", level: "单元", children: [] },
            { id: "cell-atm", name: "常压蒸馏单元", level: "单元", children: [] },
            { id: "cell-vac", name: "减压蒸馏单元", level: "单元", children: [] }
          ]
        },
        {
          id: "unit-utility",
          name: "公用工程装置",
          level: "装置",
          children: [
            { id: "cell-fuel", name: "燃料气单元", level: "单元", children: [] }
          ]
        }
      ]
    }
  ];

  /* Ordered most-recently-modified first, matching the default list order
     the drawing manager presents. Every node level owns at least one drawing
     whose modifiedAt is newer than anything in its subtree, so selecting any
     two different PBS nodes always yields a different first row. */
  var drawings = [
    {
      id: "dwg-0000",
      fileName: "PID-0000 厂区总览.dwg",
      pbsId: "plant-01",
      state: "已发布",
      version: 3,
      modifier: "张工",
      modifiedAt: "2026-10-28 09:40",
      objectCount: 6,
      symbols: [
        { kind: "管路", tag: "L-0001-01", name: "原油总管" },
        { kind: "管路", tag: "L-0002-01", name: "常压渣油总管" },
        { kind: "管路", tag: "L-0003-01", name: "减压渣油总管" },
        { kind: "设备", tag: "V-0001", name: "污油罐" },
        { kind: "仪表", tag: "PI-0001", name: "压力指示" },
        { kind: "页间连接符", tag: "OC-0001", name: "原油来向接续" }
      ]
    },
    {
      id: "dwg-1000",
      fileName: "PID-1000 常减压总流程图.dwg",
      pbsId: "unit-cdu",
      state: "已发布",
      version: 2,
      modifier: "李工",
      modifiedAt: "2026-10-27 17:05",
      objectCount: 8,
      symbols: [
        { kind: "设备", tag: "T-1001", name: "常压塔" },
        { kind: "设备", tag: "T-2001", name: "减压塔" },
        { kind: "设备", tag: "F-1001", name: "常压加热炉" },
        { kind: "设备", tag: "E-1001", name: "原油换热器" },
        { kind: "管路", tag: "L-1001-01", name: "原油进料管路" },
        { kind: "管路", tag: "L-1002-01", name: "常压塔进料管路" },
        { kind: "仪表", tag: "TI-1001", name: "温度指示" },
        { kind: "仪表回路", tag: "TIC-1001", name: "常压塔温度回路" }
      ]
    },
    {
      id: "dwg-9000",
      fileName: "PID-9000 公用工程流程图.dwg",
      pbsId: "unit-utility",
      state: "设计中",
      version: 1,
      modifier: "王工",
      modifiedAt: "2026-10-26 14:20",
      objectCount: 5,
      symbols: [
        { kind: "管路", tag: "L-9001-01", name: "蒸汽总管" },
        { kind: "管路", tag: "L-9002-01", name: "氮气总管" },
        { kind: "管路", tag: "L-9003-01", name: "工业水总管" },
        { kind: "仪表", tag: "PI-9001", name: "压力指示" },
        { kind: "仪表", tag: "TI-9001", name: "温度指示" }
      ]
    },
    {
      id: "dwg-1101",
      fileName: "PID-1101 原油换热网络.dwg",
      pbsId: "cell-heat",
      state: "已校核",
      version: 2,
      modifier: "张工",
      modifiedAt: "2026-10-25 16:42",
      objectCount: 5,
      symbols: [
        { kind: "设备", tag: "P-1001", name: "离心泵" },
        { kind: "设备", tag: "E-1001", name: "原油换热器" },
        { kind: "设备", tag: "V-1001", name: "原油缓冲罐" },
        { kind: "管路", tag: "L-1001-01", name: "原油进料管路" },
        { kind: "仪表", tag: "TI-1001", name: "温度指示" }
      ]
    },
    {
      id: "dwg-1201",
      fileName: "PID-1201 常压塔系统.dwg",
      pbsId: "cell-atm",
      state: "已校核",
      version: 2,
      modifier: "李工",
      modifiedAt: "2026-10-25 10:30",
      objectCount: 7,
      symbols: [
        { kind: "设备", tag: "T-1001", name: "常压塔" },
        { kind: "设备", tag: "E-1101", name: "常顶冷凝器" },
        { kind: "设备", tag: "V-1101", name: "常顶回流罐" },
        { kind: "管路", tag: "L-1101-01", name: "常顶油气路" },
        { kind: "管路", tag: "L-1102-01", name: "常顶回流管路" },
        { kind: "仪表", tag: "TI-1101", name: "温度指示" },
        { kind: "仪表回路", tag: "LIC-1101", name: "回流罐液位回路" }
      ]
    },
    {
      id: "dwg-2101",
      fileName: "PID-2101 减压塔系统.dwg",
      pbsId: "cell-vac",
      state: "设计中",
      version: 1,
      modifier: "王工",
      modifiedAt: "2026-10-24 15:47",
      objectCount: 7,
      symbols: [
        { kind: "设备", tag: "T-2001", name: "减压塔" },
        { kind: "设备", tag: "E-2101", name: "减顶冷凝器" },
        { kind: "设备", tag: "V-2101", name: "减顶罐" },
        { kind: "管路", tag: "L-2101-01", name: "减压塔进料管路" },
        { kind: "管路", tag: "L-2102-01", name: "减底抽出管路" },
        { kind: "仪表", tag: "TI-2101", name: "温度指示" },
        { kind: "仪表", tag: "PI-2101", name: "压力指示" }
      ]
    },
    {
      id: "dwg-1102",
      fileName: "PID-1102 原油泵区配管.dwg",
      pbsId: "cell-heat",
      state: "设计中",
      version: 1,
      modifier: "王工",
      modifiedAt: "2026-10-24 11:18",
      objectCount: 6,
      symbols: [
        { kind: "设备", tag: "P-1002", name: "原油泵" },
        { kind: "设备", tag: "P-1003", name: "原油泵" },
        { kind: "管路", tag: "L-1002-01", name: "原油泵出口管路" },
        { kind: "管路", tag: "L-1003-01", name: "原油泵出口管路" },
        { kind: "阀门", tag: "XV-1002", name: "切断阀" },
        { kind: "仪表", tag: "PI-1002", name: "压力指示" }
      ]
    },
    {
      id: "dwg-1202",
      fileName: "PID-1202 常顶回流系统.dwg",
      pbsId: "cell-atm",
      state: "设计中",
      version: 1,
      modifier: "张工",
      modifiedAt: "2026-10-23 09:12",
      objectCount: 5,
      symbols: [
        { kind: "设备", tag: "P-1101", name: "常顶回流泵" },
        { kind: "管路", tag: "L-1103-01", name: "常顶回流管路" },
        { kind: "管路", tag: "L-1104-01", name: "回流泵出口管路" },
        { kind: "阀门", tag: "LV-1101", name: "液位调节阀" },
        { kind: "仪表", tag: "LI-1101", name: "液位指示" }
      ]
    },
    {
      id: "dwg-2102",
      fileName: "PID-2102 减顶抽空系统.dwg",
      pbsId: "cell-vac",
      state: "已发布",
      version: 2,
      modifier: "李工",
      modifiedAt: "2026-10-22 13:55",
      objectCount: 4,
      symbols: [
        { kind: "设备", tag: "V-2102", name: "减顶分液罐" },
        { kind: "管路", tag: "L-2103-01", name: "减顶抽空管路" },
        { kind: "管路", tag: "L-2104-01", name: "抽空排放管路" },
        { kind: "仪表", tag: "PI-2102", name: "压力指示" }
      ]
    },
    {
      id: "dwg-9101",
      fileName: "PID-9101 燃料气分配.dwg",
      pbsId: "cell-fuel",
      state: "设计中",
      version: 1,
      modifier: "王工",
      modifiedAt: "2026-10-21 10:05",
      objectCount: 5,
      symbols: [
        { kind: "设备", tag: "V-9101", name: "燃料气分液罐" },
        { kind: "管路", tag: "L-9101-01", name: "燃料气总管" },
        { kind: "管路", tag: "L-9102-01", name: "常压炉燃料气管路" },
        { kind: "阀门", tag: "PV-9101", name: "压力调节阀" },
        { kind: "仪表", tag: "PI-9101", name: "压力指示" }
      ]
    }
  ];

  var issues = [
    {
      id: "ISS-001",
      severity: "高",
      tag: "P-1001",
      sheet: "PID-1101 原油换热网络.dwg",
      text: "设备管口 N2 与出口管路 L-1001-01 未建立连接"
    },
    {
      id: "ISS-002",
      severity: "中",
      tag: "TI-1101",
      sheet: "PID-1201 常压塔系统.dwg",
      text: "仪表未加入仪表回路 LIC-1101"
    },
    {
      id: "ISS-003",
      severity: "低",
      tag: "L-2103-01",
      sheet: "PID-2102 减顶抽空系统.dwg",
      text: "管路缺少管道等级取值"
    }
  ];

  var activity = [
    {
      time: "2026-10-28 09:40",
      actor: "张工",
      text: "发布 PID-0000 厂区总览.dwg 的第 3 版图纸版本快照"
    },
    {
      time: "2026-10-27 17:05",
      actor: "李工",
      text: "将 PID-1000 常减压总流程图.dwg 更新至第 2 版并发布"
    },
    {
      time: "2026-10-27 11:20",
      actor: "王工",
      text: "提交 PID-1101 原油换热网络.dwg 的设计规则检查待处理问题 3 项"
    },
    {
      time: "2026-10-26 14:20",
      actor: "王工",
      text: "新建 PID-9000 公用工程流程图.dwg 并挂接至公用工程装置"
    }
  ];

  var objectTotal = 0;
  var i;
  for (i = 0; i < drawings.length; i++) {
    objectTotal += drawings[i].objectCount;
  }

  window.PID_DATA = {
    project: {
      name: "常减压装置改造项目",
      drawings: drawings.length,
      objects: objectTotal,
      issues: issues.length
    },
    pbs: pbs,
    drawings: drawings,
    recentDrawings: drawings.slice(0, 4),
    activity: activity,
    issues: issues
  };
})();

/* Workbench and drawing-management screens for the P&ID platform prototype.
   Classic script, ES5, no modules/network. Requires the shell and data IIFEs
   above. Registers renderers on window.PidScreens, which the shell consults
   inside renderScreenHtml before falling back to the placeholder. */
(function () {
  "use strict";

  var Shell = window.PidShell;

  function esc(text) {
    return String(text)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }

  /* ---------- Data helpers ---------- */

  function data() {
    return (
      window.PID_DATA || {
        project: { name: "", drawings: 0, objects: 0, issues: 0 },
        pbs: [],
        drawings: [],
        recentDrawings: [],
        activity: [],
        issues: []
      }
    );
  }

  function totalObjects() {
    var list = data().drawings;
    var total = 0;
    for (var i = 0; i < list.length; i++) {
      total += list[i].objectCount || 0;
    }
    return total;
  }

  function findPbsNode(id, nodes) {
    for (var i = 0; i < nodes.length; i++) {
      var node = nodes[i];
      if (node.id === id) {
        return node;
      }
      var hit =
        node.children && node.children.length
          ? findPbsNode(id, node.children)
          : null;
      if (hit) {
        return hit;
      }
    }
    return null;
  }

  function collectNodeIds(node, acc) {
    acc[node.id] = true;
    var children = node.children || [];
    for (var i = 0; i < children.length; i++) {
      collectNodeIds(children[i], acc);
    }
    return acc;
  }

  /* Drawings attached to the node itself plus every descendant node,
     most recently modified first. */
  function drawingsFor(pbsId) {
    var node = findPbsNode(pbsId, data().pbs);
    if (!node) {
      return [];
    }
    var ids = collectNodeIds(node, {});
    var all = data().drawings;
    var list = [];
    for (var i = 0; i < all.length; i++) {
      if (ids[all[i].pbsId]) {
        list.push(all[i]);
      }
    }
    list.sort(function (a, b) {
      if (a.modifiedAt < b.modifiedAt) {
        return 1;
      }
      if (a.modifiedAt > b.modifiedAt) {
        return -1;
      }
      return 0;
    });
    return list;
  }

  /* ---------- Shared screen frame ---------- */

  function screenOpen(id, def) {
    return (
      '<section class="screen" data-screen="' +
      esc(id) +
      '">' +
      '<header class="screen__header">' +
      '<h1 class="screen__title" data-testid="screen-title">' +
      esc(def.title) +
      "</h1>" +
      '<p class="screen__desc">' +
      esc(def.desc) +
      "</p>" +
      "</header>"
    );
  }

  /* ---------- Workbench ---------- */

  function renderWorkbench(def) {
    var d = data();
    var stats = [
      { label: "项目名", value: d.project.name },
      { label: "图纸数", value: String(d.drawings.length) },
      { label: "对象数", value: String(totalObjects()) },
      { label: "待处理问题数", value: String(d.issues.length) }
    ];

    var html = screenOpen("workbench", def);

    html += '<div class="wb-stats" data-testid="overview-stats">';
    for (var i = 0; i < stats.length; i++) {
      html +=
        '<div class="wb-stat">' +
        '<span class="wb-stat__label">' +
        esc(stats[i].label) +
        "</span>" +
        '<span class="wb-stat__value">' +
        esc(stats[i].value) +
        "</span>" +
        "</div>";
    }
    html += "</div>";

    html += '<div class="wb-grid">';

    html +=
      '<section class="panel wb-panel">' +
      '<h2 class="panel__title">最近图纸</h2>' +
      '<ul class="wb-list" data-testid="recent-drawings">';
    var recent = d.recentDrawings;
    for (var r = 0; r < recent.length; r++) {
      var g = recent[r];
      html +=
        '<li class="wb-row" data-drawing-id="' +
        esc(g.id) +
        '">' +
        '<span class="wb-file">' +
        esc(g.fileName) +
        "</span>" +
        '<span class="wb-badge">' +
        esc(g.state) +
        "</span>" +
        '<span class="wb-rev">' +
        esc(g.version) +
        "</span>" +
        '<span class="wb-user">' +
        esc(g.modifier) +
        "</span>" +
        '<span class="wb-time">' +
        esc(g.modifiedAt) +
        "</span>" +
        "</li>";
    }
    html += "</ul></section>";

    html +=
      '<section class="panel wb-panel">' +
      '<h2 class="panel__title">最近动态</h2>' +
      '<ul class="wb-list" data-testid="activity">';
    var acts = d.activity;
    for (var a = 0; a < acts.length; a++) {
      html +=
        '<li class="wb-activity-row">' +
        '<span class="wb-activity__time">' +
        esc(acts[a].time) +
        "</span>" +
        '<span class="wb-activity__actor">' +
        esc(acts[a].actor) +
        "</span>" +
        '<span class="wb-activity__text">' +
        esc(acts[a].text) +
        "</span>" +
        "</li>";
    }
    html += "</ul></section>";

    html += "</div>";

    html +=
      '<section class="panel wb-panel" data-testid="quick-actions">' +
      '<h2 class="panel__title">快捷入口</h2>' +
      '<div class="quick-actions">' +
      '<a class="quick-action" href="#/screen/drawings">进入图纸管理</a>' +
      '<a class="quick-action" href="#/screen/editor">进入 P&amp;ID 编辑器</a>' +
      '<a class="quick-action" href="#/screen/bom">查看材料清单</a>' +
      "</div>" +
      "</section>";

    return html + "</section>";
  }

  /* ---------- Drawing management ---------- */

  var drawingsState = { pbsId: null, drawingId: null };

  function pbsTreeHtml(nodes) {
    var html = "<ul>";
    for (var i = 0; i < nodes.length; i++) {
      var node = nodes[i];
      html +=
        "<li>" +
        '<button type="button" class="pbs-node' +
        (node.id === drawingsState.pbsId ? " is-selected" : "") +
        '" data-testid="pbs-node" data-pbs-id="' +
        esc(node.id) +
        '" data-level="' +
        esc(node.level) +
        '">' +
        esc(node.name) +
        "</button>";
      if (node.children && node.children.length) {
        html += pbsTreeHtml(node.children);
      }
      html += "</li>";
    }
    return html + "</ul>";
  }

  var PREVIEW_SVG =
    '<svg class="dw-preview__svg" viewBox="0 0 240 140" role="img" aria-label="图纸示意">' +
    "<style>" +
    ".pid-frame{fill:none;stroke:var(--color-border-strong);stroke-width:var(--border-width);}" +
    ".pid-line{fill:none;stroke:var(--color-accent);stroke-width:var(--border-width-thick);}" +
    ".pid-eq{fill:var(--color-accent-soft);stroke:var(--color-accent);stroke-width:var(--border-width);}" +
    ".pid-inst{fill:var(--color-panel);stroke:var(--color-warning);stroke-width:var(--border-width);}" +
    ".pid-stem{fill:none;stroke:var(--color-warning);stroke-width:var(--border-width);}" +
    ".pid-tag{fill:var(--color-text-inverse-muted);font-size:var(--font-size-xs);}" +
    "</style>" +
    '<rect class="pid-frame" x="4" y="4" width="232" height="132" rx="4"/>' +
    '<rect class="pid-eq" x="16" y="55" width="44" height="60" rx="6"/>' +
    '<text class="pid-tag" x="18" y="49">V-1001</text>' +
    '<line class="pid-line" x1="60" y1="95" x2="88" y2="95"/>' +
    '<path class="pid-eq" d="M88 87 L100 95 L88 103 Z"/>' +
    '<path class="pid-eq" d="M112 87 L100 95 L112 103 Z"/>' +
    '<line class="pid-line" x1="112" y1="95" x2="136" y2="95"/>' +
    '<circle class="pid-eq" cx="150" cy="95" r="14"/>' +
    '<path class="pid-eq" d="M144 89 L156 95 L144 101 Z"/>' +
    '<text class="pid-tag" x="132" y="74">P-1001</text>' +
    '<line class="pid-line" x1="164" y1="95" x2="212" y2="95"/>' +
    '<path class="pid-eq" d="M212 87 L226 95 L212 103 Z"/>' +
    '<text class="pid-tag" x="202" y="80">OC-1001</text>' +
    '<circle class="pid-inst" cx="70" cy="52" r="9"/>' +
    '<line class="pid-stem" x1="70" y1="61" x2="70" y2="95"/>' +
    '<text class="pid-tag" x="60" y="40">TI-1001</text>' +
    "</svg>";

  function previewHtml(current, node) {
    if (!current) {
      return '<p class="dw-preview__empty">该节点下暂无图纸。</p>';
    }
    return (
      '<div class="dw-preview__name">' +
      esc(current.fileName) +
      "</div>" +
      '<dl class="dw-preview__meta">' +
      "<div><dt>生命周期状态</dt><dd>" +
      esc(current.state) +
      "</dd></div>" +
      "<div><dt>版本</dt><dd>" +
      esc(current.version) +
      "</dd></div>" +
      "<div><dt>对象数</dt><dd>" +
      esc(current.objectCount) +
      "</dd></div>" +
      "<div><dt>所在节点</dt><dd>" +
      esc(node ? node.name : "") +
      "</dd></div>" +
      "</dl>" +
      '<div class="dw-preview__thumb">' +
      PREVIEW_SVG +
      "</div>"
    );
  }

  function renderDrawings(def) {
    var d = data();
    var node = drawingsState.pbsId
      ? findPbsNode(drawingsState.pbsId, d.pbs)
      : null;
    if (!node) {
      drawingsState.pbsId = d.pbs.length ? d.pbs[0].id : null;
      node = drawingsState.pbsId
        ? findPbsNode(drawingsState.pbsId, d.pbs)
        : null;
    }

    var list = node ? drawingsFor(node.id) : [];
    var current = null;
    for (var i = 0; i < list.length; i++) {
      if (list[i].id === drawingsState.drawingId) {
        current = list[i];
        break;
      }
    }
    if (!current) {
      current = list.length ? list[0] : null;
      drawingsState.drawingId = current ? current.id : null;
    }

    if (Shell && typeof Shell.setPendingProtoMerge === "function") {
      Shell.setPendingProtoMerge("drawings", {
        symbolCount: current ? current.objectCount : 0
      });
    }

    var html = screenOpen("drawings", def);

    html +=
      '<div class="dw-toolbar" data-testid="drawings-toolbar" role="toolbar" aria-label="图纸操作">' +
      '<button type="button" class="dw-btn dw-btn--primary">新建图纸</button>' +
      '<button type="button" class="dw-btn">导入</button>' +
      '<button type="button" class="dw-btn">复制</button>' +
      '<button type="button" class="dw-btn">移动</button>' +
      '<button type="button" class="dw-btn">权限</button>' +
      '<button type="button" class="dw-btn">版本比对</button>' +
      "</div>";

    html += '<div class="dw-layout">';

    html +=
      '<aside class="panel dw-tree" data-testid="pbs-tree">' +
      '<h2 class="panel__title">PBS 层级</h2>' +
      pbsTreeHtml(d.pbs) +
      "</aside>";

    html += '<div class="dw-main">';

    html +=
      '<section class="panel dw-list">' +
      '<h2 class="panel__title">图纸清单</h2>' +
      '<div class="dw-list-scroll">' +
      '<table class="dw-table" data-testid="drawing-list">' +
      '<colgroup><col class="col-file"><col><col><col><col></colgroup>' +
      "<thead><tr>" +
      "<th>文件名</th><th>状态</th><th>版本</th><th>修改人</th><th>修改时间</th>" +
      "</tr></thead><tbody>";
    for (var r = 0; r < list.length; r++) {
      var g = list[r];
      html +=
        '<tr class="dw-row' +
        (current && g.id === current.id ? " is-selected" : "") +
        '" data-drawing-id="' +
        esc(g.id) +
        '" data-pbs-id="' +
        esc(g.pbsId) +
        '">' +
        "<td>" +
        esc(g.fileName) +
        "</td>" +
        "<td>" +
        esc(g.state) +
        "</td>" +
        "<td>" +
        esc(g.version) +
        "</td>" +
        "<td>" +
        esc(g.modifier) +
        "</td>" +
        "<td>" +
        esc(g.modifiedAt) +
        "</td>" +
        "</tr>";
    }
    html += "</tbody></table></div></section>";

    html +=
      '<section class="panel dw-preview" data-testid="drawing-preview">' +
      '<h2 class="panel__title">图纸预览</h2>' +
      previewHtml(current, node) +
      "</section>";

    html += "</div></div>";

    return html + "</section>";
  }

  /* ---------- Delegated interaction ---------- */

  function closestEl(el, selector) {
    var matcher =
      el && (el.matches || el.msMatchesSelector || el.webkitMatchesSelector);
    while (el && el.nodeType === 1) {
      if (matcher && matcher.call(el, selector)) {
        return el;
      }
      el = el.parentElement;
    }
    return null;
  }

  function rerenderDrawings() {
    var body = document.querySelector('[data-testid="screen-body"]');
    if (!body || !Shell) {
      return;
    }
    body.innerHTML = Shell.renderScreenHtml("drawings");
    Shell.publishProto("drawings");
  }

  document.addEventListener("click", function (event) {
    var target = event.target || event.srcElement;
    if (target && target.nodeType !== 1) {
      target = target.parentElement;
    }
    if (!target) {
      return;
    }
    var node = closestEl(target, '[data-testid="pbs-node"]');
    if (node) {
      var pbsId = node.getAttribute("data-pbs-id");
      if (pbsId && pbsId !== drawingsState.pbsId) {
        drawingsState.pbsId = pbsId;
        drawingsState.drawingId = null;
        rerenderDrawings();
      }
      return;
    }
    var row = closestEl(
      target,
      '[data-testid="drawing-list"] tr[data-drawing-id]'
    );
    if (row) {
      var drawingId = row.getAttribute("data-drawing-id");
      if (drawingId && drawingId !== drawingsState.drawingId) {
        drawingsState.drawingId = drawingId;
        rerenderDrawings();
      }
    }
  });

  window.PidScreens = {
    workbench: renderWorkbench,
    drawings: renderDrawings
  };
})();

/* Symbol palette screen for the P&ID platform prototype.
   Classic script, ES5, no modules/network. Extends window.PidScreens without
   replacing the renderers registered by the bundle above. */
(function () {
  "use strict";

  var Shell = window.PidShell;

  function esc(text) {
    return String(text)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  /* ---------- Symbol catalogue ---------- */

  var CATEGORIES = [
    { id: "favorites", label: "我的常用" },
    { id: "equipment", label: "设备" },
    { id: "valve", label: "阀门" },
    { id: "instrument", label: "仪表" },
    { id: "fitting", label: "管件" }
  ];

  var SYMBOLS = [
    { id: "fv-pump", name: "离心泵", category: "favorites", shape: "pump" },
    { id: "fv-vessel-v", name: "立式容器", category: "favorites", shape: "vessel" },
    { id: "fv-vessel-h", name: "卧式容器", category: "favorites", shape: "vessel-h" },
    { id: "fv-gate", name: "闸阀", category: "favorites", shape: "gate-valve" },
    { id: "fv-globe", name: "截止阀", category: "favorites", shape: "globe-valve" },
    { id: "fv-pi", name: "压力指示", category: "favorites", shape: "instrument" },
    { id: "fv-ti", name: "温度指示", category: "favorites", shape: "instrument" },
    { id: "fv-reducer", name: "大小头", category: "favorites", shape: "reducer" },

    { id: "eq-vessel", name: "立式容器", category: "equipment", shape: "vessel" },
    { id: "eq-vessel-h", name: "卧式容器", category: "equipment", shape: "vessel-h" },
    { id: "eq-pump", name: "离心泵", category: "equipment", shape: "pump" },
    { id: "eq-pump-recip", name: "往复泵", category: "equipment", shape: "pump-recip" },
    { id: "eq-exchanger", name: "换热器", category: "equipment", shape: "exchanger" },
    { id: "eq-air-cooler", name: "空冷器", category: "equipment", shape: "air-cooler" },
    { id: "eq-furnace", name: "加热炉", category: "equipment", shape: "furnace" },
    { id: "eq-reactor", name: "反应器", category: "equipment", shape: "reactor" },
    { id: "eq-compressor", name: "压缩机", category: "equipment", shape: "compressor" },
    { id: "eq-blower", name: "鼓风机", category: "equipment", shape: "blower" },
    { id: "eq-tower", name: "塔器", category: "equipment", shape: "tower" },
    { id: "eq-tank", name: "储罐", category: "equipment", shape: "tank" },

    { id: "vv-gate", name: "闸阀", category: "valve", shape: "gate-valve" },
    { id: "vv-globe", name: "截止阀", category: "valve", shape: "globe-valve" },
    { id: "vv-ball", name: "球阀", category: "valve", shape: "ball-valve" },
    { id: "vv-butterfly", name: "蝶阀", category: "valve", shape: "butterfly-valve" },
    { id: "vv-check", name: "止回阀", category: "valve", shape: "check-valve" },
    { id: "vv-control", name: "调节阀", category: "valve", shape: "control-valve" },
    { id: "vv-relief", name: "安全阀", category: "valve", shape: "relief-valve" },
    { id: "vv-trap", name: "疏水阀", category: "valve", shape: "trap" },
    { id: "vv-shutoff", name: "切断阀", category: "valve", shape: "shutoff-valve" },
    { id: "vv-reducing", name: "减压阀", category: "valve", shape: "reducing-valve" },

    { id: "in-pi", name: "压力指示", category: "instrument", shape: "instrument" },
    { id: "in-ti", name: "温度指示", category: "instrument", shape: "instrument" },
    { id: "in-fi", name: "流量指示", category: "instrument", shape: "instrument" },
    { id: "in-li", name: "液位指示", category: "instrument", shape: "instrument" },
    { id: "in-pt", name: "压力变送", category: "instrument", shape: "instrument-xmit" },
    { id: "in-tt", name: "温度变送", category: "instrument", shape: "instrument-xmit" },
    { id: "in-loop", name: "仪表回路", category: "instrument", shape: "instrument-loop" },
    { id: "in-local", name: "就地仪表", category: "instrument", shape: "instrument-local" },

    { id: "ft-elbow", name: "弯头", category: "fitting", shape: "elbow" },
    { id: "ft-tee", name: "三通", category: "fitting", shape: "tee" },
    { id: "ft-reducer", name: "大小头", category: "fitting", shape: "reducer" },
    { id: "ft-flange", name: "法兰", category: "fitting", shape: "flange" },
    { id: "ft-blind", name: "盲板", category: "fitting", shape: "blind" },
    { id: "ft-sight", name: "视镜", category: "fitting", shape: "sight-glass" },
    { id: "ft-filter", name: "过滤器", category: "fitting", shape: "filter" },
    { id: "ft-arrester", name: "阻火器", category: "fitting", shape: "flame-arrester" },
    { id: "ft-hose", name: "软管接头", category: "fitting", shape: "hose" }
  ];

  /* Thumbnail geometry keyed by shape id. Every glyph is a stroke drawing on
     a shared 48x48 grid; colour and stroke width come from the
     .pal__thumb svg rule (currentColor + border tokens). Unknown shapes
     fall back to the generic outline so a glyph can never be blank. */
  var SHAPES = {
    "generic":
      '<rect x="10" y="10" width="28" height="28" rx="4"/>' +
      '<line x1="10" y1="24" x2="38" y2="24"/>',
    "vessel":
      '<rect x="16" y="6" width="16" height="32" rx="8"/>' +
      '<line x1="18" y1="38" x2="18" y2="44"/>' +
      '<line x1="30" y1="38" x2="30" y2="44"/>',
    "vessel-h":
      '<rect x="6" y="16" width="36" height="16" rx="8"/>' +
      '<line x1="14" y1="32" x2="14" y2="38"/>' +
      '<line x1="34" y1="32" x2="34" y2="38"/>',
    "pump":
      '<circle cx="24" cy="24" r="13"/>' +
      '<path d="M18 18 L32 24 L18 30 Z"/>',
    "pump-recip":
      '<rect x="8" y="18" width="18" height="12"/>' +
      '<line x1="26" y1="24" x2="33" y2="24"/>' +
      '<circle cx="37" cy="24" r="4"/>',
    "exchanger":
      '<rect x="10" y="16" width="28" height="16"/>' +
      '<path d="M14 28 L20 20 L26 28 L32 20"/>',
    "air-cooler":
      '<rect x="8" y="18" width="32" height="14"/>' +
      '<circle cx="24" cy="25" r="5"/>' +
      '<line x1="24" y1="18" x2="24" y2="32"/>' +
      '<line x1="19" y1="25" x2="29" y2="25"/>',
    "furnace":
      '<rect x="12" y="8" width="24" height="32"/>' +
      '<path d="M24 32 C20 28 22 24 24 19 C26 24 28 28 24 32 Z"/>',
    "reactor":
      '<rect x="16" y="12" width="16" height="26" rx="6"/>' +
      '<line x1="24" y1="4" x2="24" y2="30"/>' +
      '<line x1="18" y1="30" x2="30" y2="30"/>',
    "compressor":
      '<path d="M10 16 L36 21 L36 27 L10 32 Z"/>' +
      '<line x1="36" y1="24" x2="44" y2="24"/>',
    "blower":
      '<path d="M10 21 L36 16 L36 32 L10 27 Z"/>' +
      '<line x1="4" y1="24" x2="10" y2="24"/>',
    "tower":
      '<rect x="18" y="6" width="12" height="32"/>' +
      '<line x1="18" y1="16" x2="30" y2="16"/>' +
      '<line x1="18" y1="25" x2="30" y2="25"/>' +
      '<line x1="20" y1="38" x2="20" y2="43"/>' +
      '<line x1="28" y1="38" x2="28" y2="43"/>',
    "tank":
      '<path d="M10 15 L24 8 L38 15"/>' +
      '<line x1="10" y1="15" x2="10" y2="38"/>' +
      '<line x1="38" y1="15" x2="38" y2="38"/>' +
      '<line x1="10" y1="38" x2="38" y2="38"/>',
    "gate-valve":
      '<path d="M10 18 L24 24 L10 30 Z"/>' +
      '<path d="M38 18 L24 24 L38 30 Z"/>' +
      '<line x1="24" y1="12" x2="24" y2="36"/>' +
      '<line x1="18" y1="12" x2="30" y2="12"/>',
    "globe-valve":
      '<path d="M10 18 L24 24 L10 30 Z"/>' +
      '<path d="M38 18 L24 24 L38 30 Z"/>' +
      '<circle cx="24" cy="24" r="4"/>',
    "ball-valve":
      '<path d="M10 18 L24 24 L10 30 Z"/>' +
      '<path d="M38 18 L24 24 L38 30 Z"/>' +
      '<circle cx="24" cy="24" r="5"/>' +
      '<line x1="19" y1="24" x2="29" y2="24"/>',
    "butterfly-valve":
      '<path d="M10 18 L24 24 L10 30 Z"/>' +
      '<path d="M38 18 L24 24 L38 30 Z"/>' +
      '<line x1="24" y1="24" x2="31" y2="12"/>',
    "check-valve":
      '<path d="M12 18 L26 24 L12 30 Z"/>' +
      '<line x1="30" y1="17" x2="30" y2="31"/>' +
      '<line x1="36" y1="17" x2="36" y2="31"/>',
    "control-valve":
      '<path d="M10 18 L24 24 L10 30 Z"/>' +
      '<path d="M38 18 L24 24 L38 30 Z"/>' +
      '<line x1="24" y1="24" x2="24" y2="14"/>' +
      '<path d="M18 14 A6 6 0 0 1 30 14 Z"/>',
    "relief-valve":
      '<path d="M10 18 L24 24 L10 30 Z"/>' +
      '<path d="M38 18 L24 24 L38 30 Z"/>' +
      '<line x1="24" y1="24" x2="24" y2="16"/>' +
      '<path d="M20 16 L28 13 L20 10 L28 7"/>',
    "trap":
      '<circle cx="24" cy="24" r="11"/>' +
      '<path d="M17 20 L31 20 L24 30 Z"/>',
    "shutoff-valve":
      '<path d="M10 18 L24 24 L10 30 Z"/>' +
      '<path d="M38 18 L24 24 L38 30 Z"/>' +
      '<line x1="24" y1="24" x2="24" y2="16"/>' +
      '<rect x="19" y="8" width="10" height="8"/>',
    "reducing-valve":
      '<path d="M10 18 L24 24 L10 30 Z"/>' +
      '<path d="M38 18 L24 24 L38 30 Z"/>' +
      '<line x1="24" y1="24" x2="24" y2="15"/>' +
      '<circle cx="24" cy="11" r="4"/>',
    "instrument":
      '<circle cx="24" cy="24" r="12"/>' +
      '<line x1="12" y1="24" x2="36" y2="24"/>',
    "instrument-xmit":
      '<circle cx="24" cy="24" r="12"/>' +
      '<line x1="12" y1="21" x2="36" y2="21"/>' +
      '<line x1="12" y1="27" x2="36" y2="27"/>',
    "instrument-loop":
      '<circle cx="24" cy="20" r="11"/>' +
      '<line x1="13" y1="20" x2="35" y2="20"/>' +
      '<path d="M24 31 L29 38 L24 45 L19 38 Z"/>',
    "instrument-local":
      '<circle cx="24" cy="18" r="10"/>' +
      '<line x1="14" y1="18" x2="34" y2="18"/>' +
      '<line x1="24" y1="28" x2="24" y2="40"/>' +
      '<line x1="8" y1="40" x2="40" y2="40"/>',
    "elbow":
      '<path d="M6 20 L22 20 A8 8 0 0 1 30 28 L30 42"/>',
    "tee":
      '<line x1="6" y1="20" x2="42" y2="20"/>' +
      '<line x1="24" y1="20" x2="24" y2="38"/>' +
      '<line x1="20" y1="38" x2="28" y2="38"/>',
    "reducer":
      '<path d="M8 16 L30 21 L30 27 L8 32 Z"/>' +
      '<line x1="6" y1="24" x2="8" y2="24"/>' +
      '<line x1="30" y1="24" x2="42" y2="24"/>',
    "flange":
      '<line x1="6" y1="24" x2="42" y2="24"/>' +
      '<line x1="19" y1="15" x2="19" y2="33"/>' +
      '<line x1="29" y1="15" x2="29" y2="33"/>',
    "blind":
      '<line x1="6" y1="24" x2="32" y2="24"/>' +
      '<line x1="32" y1="14" x2="32" y2="34"/>' +
      '<line x1="37" y1="14" x2="37" y2="34"/>',
    "sight-glass":
      '<line x1="6" y1="24" x2="42" y2="24"/>' +
      '<line x1="16" y1="16" x2="16" y2="32"/>' +
      '<line x1="32" y1="16" x2="32" y2="32"/>' +
      '<circle cx="24" cy="24" r="5"/>',
    "filter":
      '<line x1="6" y1="18" x2="42" y2="18"/>' +
      '<path d="M18 18 L26 38 L34 18"/>',
    "flame-arrester":
      '<line x1="6" y1="24" x2="42" y2="24"/>' +
      '<rect x="17" y="13" width="14" height="22"/>' +
      '<line x1="22" y1="13" x2="22" y2="35"/>' +
      '<line x1="26" y1="13" x2="26" y2="35"/>',
    "hose":
      '<path d="M4 24 C10 14 16 34 22 24 C28 14 34 34 40 24"/>' +
      '<line x1="42" y1="17" x2="42" y2="31"/>'
  };

  function drawSymbol(shape) {
    var inner = Object.prototype.hasOwnProperty.call(SHAPES, shape)
      ? SHAPES[shape]
      : SHAPES.generic;
    return (
      '<svg viewBox="0 0 48 48" fill="none" stroke="currentColor" aria-hidden="true">' +
      inner +
      "</svg>"
    );
  }

  /* ---------- Screen state & rendering ---------- */

  var state = { category: "favorites", keyword: "" };

  function categoryLabel(id) {
    for (var i = 0; i < CATEGORIES.length; i++) {
      if (CATEGORIES[i].id === id) {
        return CATEGORIES[i].label;
      }
    }
    return id;
  }

  /* Symbols in the active category filtered by the current keyword.
     Matching is a case-insensitive substring test on the Chinese name. */
  function currentItems() {
    var items = [];
    var kw = String(state.keyword || "").toLowerCase();
    for (var i = 0; i < SYMBOLS.length; i++) {
      var s = SYMBOLS[i];
      if (s.category !== state.category) {
        continue;
      }
      if (kw && s.name.toLowerCase().indexOf(kw) === -1) {
        continue;
      }
      items.push(s);
    }
    return items;
  }

  function renderPalette(def) {
    var items = currentItems();

    if (Shell && typeof Shell.setPendingProtoMerge === "function") {
      Shell.setPendingProtoMerge("palette", {
        paletteItemCount: items.length
      });
    }

    var html =
      '<section class="screen" data-screen="palette">' +
      '<header class="screen__header">' +
      '<h1 class="screen__title" data-testid="screen-title">' +
      esc(def.title) +
      "</h1>" +
      '<p class="screen__desc">' +
      esc(def.desc) +
      "</p>" +
      "</header>";

    html += '<div class="panel pal">';

    html += '<div class="pal__toolbar">';
    html += '<div class="pal__tabs" data-testid="palette-tabs" role="tablist">';
    for (var c = 0; c < CATEGORIES.length; c++) {
      var cat = CATEGORIES[c];
      var active = cat.id === state.category;
      html +=
        '<button type="button" class="pal__tab' +
        (active ? " is-active" : "") +
        '" data-testid="palette-tab" data-category="' +
        esc(cat.id) +
        '" role="tab" aria-selected="' +
        (active ? "true" : "false") +
        '">' +
        esc(cat.label) +
        "</button>";
    }
    html += "</div>";
    html +=
      '<input class="field pal__search" data-testid="palette-search" type="search"' +
      ' placeholder="搜索符号名称" aria-label="搜索符号" value="' +
      esc(state.keyword) +
      '">';
    html += "</div>";

    html +=
      '<p class="pal__summary" data-testid="palette-summary">当前分类：' +
      esc(categoryLabel(state.category)) +
      " · 共 " +
      items.length +
      " 个符号</p>";

    html += '<div class="pal__grid" data-testid="palette-grid">';
    for (var i = 0; i < items.length; i++) {
      var s = items[i];
      html +=
        '<button type="button" class="pal__item" data-testid="palette-item"' +
        ' data-symbol-id="' +
        esc(s.id) +
        '" title="' +
        esc(s.name) +
        '">' +
        '<span class="pal__thumb">' +
        drawSymbol(s.shape) +
        "</span>" +
        '<span class="pal__name">' +
        esc(s.name) +
        "</span>" +
        "</button>";
    }
    html += "</div>";

    html += "</div></section>";
    return html;
  }

  /* ---------- Delegated interaction ---------- */

  function closestEl(el, selector) {
    var matcher =
      el && (el.matches || el.msMatchesSelector || el.webkitMatchesSelector);
    while (el && el.nodeType === 1) {
      if (matcher && matcher.call(el, selector)) {
        return el;
      }
      el = el.parentElement;
    }
    return null;
  }

  /* Guards every delegated listener so clicks and keystrokes on other
     screens are never mistaken for palette input. */
  function paletteActive() {
    if (
      Shell &&
      typeof Shell.currentScreenId === "function" &&
      Shell.currentScreenId() === "palette"
    ) {
      return true;
    }
    return !!document.querySelector('[data-screen="palette"]');
  }

  function rerenderPalette(refocusSearch) {
    var body = document.querySelector('[data-testid="screen-body"]');
    if (!body || !Shell) {
      return;
    }
    body.innerHTML = Shell.renderScreenHtml("palette");
    Shell.publishProto("palette");
    if (refocusSearch) {
      var input = body.querySelector('[data-testid="palette-search"]');
      if (input) {
        input.focus();
      }
    }
  }

  document.addEventListener("click", function (event) {
    if (!paletteActive()) {
      return;
    }
    var target = event.target || event.srcElement;
    if (target && target.nodeType !== 1) {
      target = target.parentElement;
    }
    if (!target) {
      return;
    }
    var tab = closestEl(target, '[data-testid="palette-tab"]');
    if (tab) {
      var category = tab.getAttribute("data-category");
      if (category && category !== state.category) {
        state.category = category;
        state.keyword = "";
        rerenderPalette(false);
      }
    }
  });

  document.addEventListener("input", function (event) {
    if (!paletteActive()) {
      return;
    }
    var target = event.target || event.srcElement;
    if (!target || target.nodeType !== 1) {
      return;
    }
    var input = closestEl(target, '[data-testid="palette-search"]');
    if (!input) {
      return;
    }
    state.keyword = input.value || "";
    rerenderPalette(true);
  });

  window.PidScreens = window.PidScreens || {};
  window.PidScreens.palette = renderPalette;
})();
