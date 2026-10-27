/* Boot script and hash router for the prototype shell.
   Classic script, ES5, no modules, no network. Requires screens-shell.js. */
(function () {
  "use strict";

  var Shell = window.PidShell;

  function parseScreenId(hash) {
    var match = /^#\/screen\/([A-Za-z0-9-]+)\/?$/.exec(hash || "");
    return match ? match[1] : null;
  }

  function markActive(screenId) {
    var links = document.querySelectorAll(".app-shell a[data-screen]");
    for (var i = 0; i < links.length; i++) {
      var isActive = links[i].getAttribute("data-screen") === screenId;
      if (isActive) {
        links[i].classList.add("is-active");
        links[i].setAttribute("aria-current", "page");
      } else {
        links[i].classList.remove("is-active");
        links[i].removeAttribute("aria-current");
      }
    }
  }

  function renderScreen(screenId) {
    var body = document.querySelector('[data-testid="screen-body"]');
    if (body) {
      body.innerHTML = Shell.renderScreenHtml(screenId);
    }
    markActive(screenId);
    Shell.publishProto(screenId);
    var def = Shell.MAP[screenId];
    if (def) {
      document.title = def.title + " · 智能 P&ID 平台";
    }
    var routeEl = document.querySelector("[data-status-route]");
    if (routeEl) {
      routeEl.textContent = Shell.ROUTE_PREFIX + screenId;
    }
  }

  function route() {
    if (!Shell) {
      return;
    }
    var screenId = parseScreenId(window.location.hash);
    if (!screenId || !Shell.MAP[screenId]) {
      /* Empty or unknown hash: replace, never render an error page. */
      window.location.replace(Shell.ROUTE_PREFIX + Shell.DEFAULT_SCREEN);
      screenId = Shell.DEFAULT_SCREEN;
    }
    renderScreen(screenId);
  }

  function boot() {
    if (!Shell) {
      return;
    }
    var nav = document.querySelector('[data-testid="module-nav"]');
    if (nav) {
      Shell.buildModuleNav(nav);
    }
    route();
    var status = document.querySelector('[data-testid="boot-status"]');
    if (status) {
      status.textContent = "外壳就绪";
    }
    if (document.body) {
      document.body.dataset.boot = "ready";
    }
  }

  window.addEventListener("hashchange", route);

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", boot);
  } else {
    boot();
  }
})();
