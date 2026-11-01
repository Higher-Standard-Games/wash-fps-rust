const { open, boot, waitForMap, sleep } = require('./lib.cjs');
// usage: views.cjs <demo> <prefix> "<x,y,z,yawDeg,pitchDeg>;..." [WxH] [keep regex] ["<x,y,z,yawDeg,clip,frame>;..."]
// Q3 position and angles (pitch: positive looks down). Writes <prefix>_<i>.png.
// Map weapon pickups are shown (all of them, as if available), as washoi view
// draws them; every other mesh the keep regex does not name is hidden.
// Players (the last argument) stand at Quake origins as Vanguard models in
// the FFA style, their clip held at a frame, as washoi view --player draws them.
(async () => {
  setTimeout(() => process.exit(3), 1_500_000).unref();
  const [demo, prefix, list, size = '1280x720', keepRe = '^(bsp|skybox)', players = ''] = process.argv.slice(2);
  const [w, h] = size.split('x').map(Number);
  const b = await open({ width: w, height: h });
  try {
    await boot(b);
    await b.command(`demo ${demo}`);
    b.command('demoplay');
    await waitForMap(b);
    await sleep(2000);
    await b.ev(async ([keepRe, players]) => {
      const { DemoService } = await import('/src/games/fps/client/DemoService.ts');
      DemoService.getInstance().activeDemoViewer?.pause();
      const scene = window.scene;
      const camera = scene.activeCameras?.[0] ?? scene.activeCamera;
      const keep = new RegExp(`${keepRe}|^probe_player`);
      if (players) {
        const { PlayerModelManager } = await import('/src/engine/animation/PlayerModelManager.ts');
        const { getPlayerModelVisualStyle } = await import('/src/games/fps/client/PlayerModelVisualSettings.ts');
        const manager = new PlayerModelManager(scene);
        await manager.loadModel();
        for (const [i, p] of players.split(';').entries()) {
          const [x, y, z, angle, clip = 'idle', frame = '0'] = p.split(',');
          const inst = manager.createInstance(`probe_player${i}`, 36);
          // RemotePlayerRenderer.updateModel: anchor at the feet, turned by yaw.
          inst.anchor.position.set(Number(x), Number(z) - 24, Number(y));
          inst.anchor.rotation.set(0, ((90 - Number(angle)) * Math.PI) / 180, 0);
          manager.applyVisualStyle(inst, getPlayerModelVisualStyle('ffa', 36));
          manager.playAnimation(inst, clip);
          inst.masterGroup.goToFrame(Number(frame));
          inst.masterGroup.pause();
          manager.resetRootMotion(inst);
        }
      }
      const { FPSPickup } = await import('/src/games/fps/world/FPSPickup.ts');
      const weapons = new Set();
      for (const p of FPSPickup.allPickups) {
        if (p.options.type !== 'weapon' || !p.mesh) continue;
        for (const m of [p.mesh, ...p.mesh.getChildMeshes(false)]) weapons.add(m);
      }
      window.__probeView = [0, 0, 0, 0, 0];
      const place = () => {
        const [x, y, z, yaw, pitch] = window.__probeView;
        camera.position.set(x, z, y);
        if (camera.rotationQuaternion) camera.rotationQuaternion = null;
        camera.rotation.set((pitch * Math.PI) / 180, ((90 - yaw) * Math.PI) / 180, 0);
        camera.fov = 0.8;
        for (const m of scene.meshes) if (!keep.test(m.name) && !weapons.has(m) && m.isVisible) m.isVisible = false;
        // The paused demo leaves taken pickups disabled, and a pickup hides its
        // model from a camera whose cluster cannot see it, judged before this
        // camera moved (Pickup.applyModelInView).
        for (const p of FPSPickup.allPickups) if (p.options.type === 'weapon' && p.mesh && !p.mesh.isEnabled()) p.mesh.setEnabled(true);
        for (const m of weapons) if (m.getTotalVertices() > 0) m.isVisible = true;
      };
      scene.onBeforeCameraRenderObservable.add(place);
      for (const el of document.body.querySelectorAll('body > *')) if (!el.querySelector('canvas') && el.tagName !== 'CANVAS') el.style.visibility = 'hidden';
    }, [keepRe, players]);
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
