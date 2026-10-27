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

  function publishProto(screenId) {
    currentScreenId = screenId;
    var proto = protoSeed();
    proto.screen = screenId;
    window.__proto = proto;
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
    renderScreenHtml: renderScreenHtml,
    buildModuleNav: buildModuleNav,
    currentScreenId: function () {
      return currentScreenId;
    }
  };

  /* Publish the seeded surface immediately so window.__proto always exists. */
  publishProto("");
})();
