const { open, boot, waitForMap, sleep } = require('./lib.cjs');
// usage: views.cjs <demo> <prefix> "<x,y,z,yawDeg,pitchDeg>;..." [WxH] [keep regex]
// Q3 position and angles (pitch: positive looks down). Writes <prefix>_<i>.png.
(async () => {
  setTimeout(() => process.exit(3), 1_500_000).unref();
  const [demo, prefix, list, size = '1280x720', keepRe = '^(bsp|skybox)'] = process.argv.slice(2);
  const [w, h] = size.split('x').map(Number);
  const b = await open({ width: w, height: h });
  try {
    await boot(b);
    await b.command(`demo ${demo}`);
    b.command('demoplay');
    await waitForMap(b);
    await sleep(2000);
    await b.ev(async (keepRe) => {
      const { DemoService } = await import('/src/games/fps/client/DemoService.ts');
      DemoService.getInstance().activeDemoViewer?.pause();
      const scene = window.scene;
      const camera = scene.activeCameras?.[0] ?? scene.activeCamera;
      const keep = new RegExp(keepRe);
      window.__probeView = [0, 0, 0, 0, 0];
      const place = () => {
        const [x, y, z, yaw, pitch] = window.__probeView;
        camera.position.set(x, z, y);
        if (camera.rotationQuaternion) camera.rotationQuaternion = null;
        camera.rotation.set((pitch * Math.PI) / 180, ((90 - yaw) * Math.PI) / 180, 0);
        camera.fov = 0.8;
        for (const m of scene.meshes) if (!keep.test(m.name) && m.isVisible) m.isVisible = false;
      };
      scene.onBeforeCameraRenderObservable.add(place);
      for (const el of document.body.querySelectorAll('body > *')) if (!el.querySelector('canvas') && el.tagName !== 'CANVAS') el.style.visibility = 'hidden';
    }, keepRe);
    for (const [i, at] of list.split(';').entries()) {
      await b.ev((v) => { window.__probeView = v; }, at.split(',').map(Number));
      await sleep(5000);
      await b.page.screenshot({ path: `${prefix}_${i}.png` });
      console.log('wrote', `${prefix}_${i}.png`);
    }
  } finally {
    console.log(b.logs.filter((l) => /EXCEPTION/i.test(l)).slice(0, 8).join('\n'));
    await b.close();
  }
})();
