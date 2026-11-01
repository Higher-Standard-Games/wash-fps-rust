const { open, boot, waitForMap, sleep } = require('./lib.cjs');
// usage: views.cjs <demo> <prefix> "<x,y,z,yawDeg,pitchDeg>;..." [WxH] [keep regex] ["<x,y,z,yawDeg,clip,frame>;..."] [item seconds] ["<id,x,y,z,yawDeg,pitchDeg>;..."]
// Q3 position and angles (pitch: positive looks down). Writes <prefix>_<i>.png.
// Map weapon pickups are shown (all of them, as if available), as washoi view
// draws them; every other mesh the keep regex does not name is hidden.
// Players (the last argument) stand at Quake origins as Vanguard models in
// the FFA style, their clip held at a frame, as washoi view --player draws them.
// With item seconds, every procedural pickup is shown, available and in view,
// posed that many seconds after it was built, as washoi view --time draws it.
// Projectiles (the last argument) are held at Quake origins, flying along a
// Quake yaw and pitch, without their light or trail, their shader stages that
// many item seconds in, as washoi view --projectile draws them.
(async () => {
  setTimeout(() => process.exit(3), 1_500_000).unref();
  const [demo, prefix, list, size = '1280x720', keepRe = '^(bsp|skybox)', players = '', itemTime = '', projectiles = ''] = process.argv.slice(2);
  const [w, h] = size.split('x').map(Number);
  const b = await open({ width: w, height: h });
  try {
    await boot(b);
    await b.command(`demo ${demo}`);
    b.command('demoplay');
    await waitForMap(b);
    await sleep(2000);
    await b.ev(async ([keepRe, players, itemTime, projectiles]) => {
      const { DemoService } = await import('/src/games/fps/client/DemoService.ts');
      DemoService.getInstance().activeDemoViewer?.pause();
      const scene = window.scene;
      const camera = scene.activeCameras?.[0] ?? scene.activeCamera;
      const keep = new RegExp(`${keepRe}|^probe_player${itemTime ? '|^pp_' : ''}${projectiles ? '|^proj_\\d+_(?!trail)' : ''}`);
      const frozen = [];
      if (projectiles) {
        const { Vector3 } = await import('/node_modules/@babylonjs/core/Maths/math.vector.js');
        const { getProjectileManager } = await import('/src/engine/weapon/ProjectileManager.ts');
        const { ShaderAnimationSystem } = await import('/src/engine/loader/shader/ShaderAnimationSystem.ts');
        const { createTcState, stepTcModsInto, evaluateWaveFunc } = await import('/src/engine/loader/shader/Q3StageEvaluation.ts');
        const manager = getProjectileManager();
        const ids = [];
        for (const [i, p] of projectiles.split(';').entries()) {
          const [id, x, y, z, yaw, pitch = '0'] = p.split(',');
          await manager.preloadModel(id);
          const ya = (Number(yaw) * Math.PI) / 180;
          const pa = (Number(pitch) * Math.PI) / 180;
          const dir = new Vector3(Math.cos(ya) * Math.cos(pa), -Math.sin(pa), Math.sin(ya) * Math.cos(pa));
          manager.spawn(id, new Vector3(Number(x), Number(z), Number(y)), dir, { isServerDriven: true, serverEntityId: `probe${i}` });
          ids.push(id);
        }
        for (const proj of manager.projectiles) {
          if (!proj.serverEntityId?.startsWith('probe')) continue;
          proj.gravity = 0;
          proj.light?.setEnabled(false);
          if (proj.light) proj.light.intensity = 0;
          for (const m of proj.fadeMeshes) m.visibility = 1;
          proj.spawnTime = -1e9;
        }
        // The shader stages of the projectiles' materials, posed at the item
        // time as washoi view poses them (stepped once over the whole time).
        const T = Number(itemTime || 0);
        for (const entry of ShaderAnimationSystem.getInstance().entries.values()) {
          if (!ids.some((id) => entry.id.startsWith(`${id}_`))) continue;
          frozen.push(entry);
        }
        window.__freezeStages = () => {
          for (const entry of frozen) for (const stage of entry.stages) {
            const state = createTcState(stage.tcMods);
            if (stage.tcMods.length) {
              stepTcModsInto(state, stage.tcMods, T, T, () => 0.5);
              stage.texture.uOffset = state.uOffset;
              stage.texture.vOffset = state.vOffset;
              stage.texture.uScale = state.uScale;
              stage.texture.vScale = state.vScale;
              stage.texture.wAng = state.rotation;
            }
            if (stage.rgbGen?.type === 'wave' && stage.rgbGen.wave && stage.colorTarget === 'emissive') {
              stage.texture.level = Math.max(0, Math.min(1, evaluateWaveFunc(stage.rgbGen.wave, T, () => 0.5)));
            }
          }
        };
      }
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
      const owners = itemTime ? [...FPSPickup.allPickups].filter((p) => p.visual) : [];
      // A pickup judges its visual in view from the camera before it moved.
      for (const p of owners) p.isInView = () => true;
      const items = owners.map((p) => p.visual);
      const place = () => {
        window.__freezeStages?.();
        for (const v of items) {
          v.visible = true;
          v.inView = true;
          v.assembleAge = Number.POSITIVE_INFINITY;
          v.cosmetic.setEnabled(true);
          v.update(0, Number(itemTime));
        }
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
    }, [keepRe, players, itemTime, projectiles]);
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
