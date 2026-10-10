(() => {
  "use strict";

  const selectors = [
    "ins.adsbygoogle",
    "iframe[src*='doubleclick.net']",
    "iframe[src*='googlesyndication.com']",
    "[id^='google_ads']",
    "[id^='div-gpt-ad']",
    "[class~='ad-banner']",
    "[class~='ad-container']",
    "[class~='advertisement']",
    "[data-ad-slot]",
    "[data-ad-unit]",
  ];

  const style = document.createElement("style");
  style.textContent = selectors.join(",\n") + " { display: none !important; }";

  (document.head || document.documentElement).appendChild(style);

  function cleanAds(root) {
    if (!(root instanceof Element || root instanceof Document)) return;

    for (const selector of selectors) {
      try {
        if (root.matches?.(selector))
          root.style.setProperty("display", "none", "important");
        root.querySelectorAll?.(selector).forEach((el) => {
          el.style.setProperty("display", "none", "important");
        });
      } catch (_) {}
    }
  }

  cleanAds(document);

  const observer = new MutationObserver((records) => {
    for (const record of records) {
      for (const node of record.addedNodes) {
        if (node.nodeType === Node.ELEMENT_NODE) cleanAds(node);
      }
    }
  });

  observer.observe(document.documentElement, {
    childList: true,
    subtree: true,
  });
})();
