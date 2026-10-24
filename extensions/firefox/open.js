// Handles ext+flipper:container=N&url=URL links that Flipper passes on the
// command line, since Firefox cannot open a container from there.
(async () => {
  const status = document.getElementById("status");
  try {
    let link = location.hash.slice(1);
    if (!/^ext\+flipper:/i.test(link)) link = decodeURIComponent(link);
    const params = new URLSearchParams(link.replace(/^ext\+flipper:/i, ""));
    const container = params.get("container") || "";
    const url = params.get("url") || "";
    if (!/^[1-9][0-9]*$/.test(container) || !/^https?:/i.test(url)) {
      throw new Error("invalid Flipper link");
    }
    const current = await browser.tabs.getCurrent();
    await browser.tabs.create({
      url,
      cookieStoreId: `firefox-container-${container}`,
      windowId: current.windowId,
      index: current.index + 1,
    });
    await browser.tabs.remove(current.id);
  } catch (error) {
    status.textContent = `Flipper could not open this link: ${error.message}`;
  }
})();
