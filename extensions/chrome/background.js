const menuId = "flipper-open-link";

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: menuId,
    title: "Open link with Flipper",
    contexts: ["link", "page"],
  });
});

chrome.contextMenus.onClicked.addListener((info) => {
  if (info.menuItemId !== menuId) return;
  const url = info.linkUrl || info.pageUrl;
  if (!url || !/^(https?:|file:)/i.test(url)) return;
  chrome.runtime.sendNativeMessage("com.korora.flipper", { url }, (response) => {
    if (chrome.runtime.lastError) console.error(chrome.runtime.lastError.message);
    else if (!response?.ok) console.error(response?.error || "Flipper could not open the link");
  });
});
