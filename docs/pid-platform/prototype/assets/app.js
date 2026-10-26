/* Boot script for the prototype shell. ES5 only: no modules, no network. */
(function () {
  "use strict";

  function boot() {
    if (document.body) {
      document.body.dataset.boot = "ready";
    }
    var status = document.querySelector('[data-testid="boot-status"]');
    if (status) {
      status.textContent = "外壳就绪";
    }
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", boot);
  } else {
    boot();
  }
})();
