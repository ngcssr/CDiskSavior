// 生成应用品牌图标：SVG → 1024 PNG（透明背景）
const { chromium } = require("playwright");
const path = require("path");

(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage({
    viewport: { width: 1024, height: 1024 },
    deviceScaleFactor: 1,
  });
  const svgPath = "file:///" + path.resolve(__dirname, "brand.svg").replace(/\\/g, "/");
  await page.goto(svgPath);
  await page.screenshot({
    path: path.resolve(__dirname, "icon-master.png"),
    omitBackground: true,
  });
  await browser.close();
  console.log("icon-master.png generated");
})();
