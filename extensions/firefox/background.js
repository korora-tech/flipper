browser.contextMenus.create({
  id: "flipper-open-link",
  title: "Open link with Flipper",
  contexts: ["link", "page"],
});

browser.contextMenus.onClicked.addListener((info) => {
  if (info.menuItemId !== "flipper-open-link") return;
  const url = info.linkUrl || info.pageUrl;
  if (!url || !/^(https?:|file:)/i.test(url)) return;
  browser.runtime.sendNativeMessage("com.korora.flipper", { url }).catch(console.error);
});
