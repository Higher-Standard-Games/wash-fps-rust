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
// Quake yaw and pitch, without their light, their shader stages that many
// item seconds in, as washoi view --projectile draws them. A trail is the one
// a straight second of flight at 60 frames a second leaves, rebuilt each frame
// for the camera. A weapon id (the last argument) holds that first-person
// weapon at the hip, as washoi view --weapon draws it.
(async () => {
  setTimeout(() => process.exit(3), 1_500_000).unref();
  const [demo, prefix, list, size = '1280x720', keepRe = '^(bsp|skybox)', players = '', itemTime = '', projectiles = '', explosions = '', effects = '', weapon = ''] = process.argv.slice(2);
  const [w, h] = size.split('x').map(Number);
  const b = await open({ width: w, height: h });
  try {
    await boot(b);
    await b.command(`demo ${demo}`);
    b.command('demoplay');
    await waitForMap(b);
    await sleep(2000);
    const lights = process.env.LIGHTS ? '1' : '';
    await b.ev(async ([keepRe, players, itemTime, projectiles, explosions, effects, lights, weapon]) => {
      const { DemoService } = await import('/src/games/fps/client/DemoService.ts');
      DemoService.getInstance().activeDemoViewer?.pause();
      const scene = window.scene;
      const camera = scene.activeCameras?.[0] ?? scene.activeCamera;
      const keep = new RegExp(`${keepRe}|^probe_player${itemTime ? '|^pp_' : ''}${projectiles ? '|^proj_\\d+_' : ''}${explosions ? '|^explosion_' : ''}${effects ? '|^(tracer_streak|muzzle_flash|impact_spark|rail_beam|rail_impact|voltaic_.*)$' : ''}`);
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
          if (!lights) {
            proj.light?.setEnabled(false);
            if (proj.light) proj.light.intensity = 0;
          }
          for (const m of proj.fadeMeshes) m.visibility = 1;
          proj.spawnTime = -1e9;
        }
        const { getEntityRegistry } = await import('/src/shared/entities/index.ts');
        window.__holdTrails = () => {
          camera.computeWorldMatrix(true);
          for (const proj of manager.projectiles) {
            if (!proj.serverEntityId?.startsWith('probe')) continue;
            const def = getEntityRegistry().get(proj.defId);
            if (!def?.trailEffect) continue;
            const step = proj.velocity.length() / 60;
            const head = proj.node.position;
            proj.trailPoints = Array.from({ length: 60 }, (_, k) => head.subtract(proj.direction.scale(step * (60 - k))));
            manager.updateTrail(proj, def.trailLength);
          }
        };
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
      if (explosions) {
        // Each explosion from mulberry32 seed 1 in place of Math.random, as
        // washoi view spawns it, at its age on a held clock, without its
        // flash light unless LIGHTS is set.
        const { Vector3 } = await import('/node_modules/@babylonjs/core/Maths/math.vector.js');
        const fx = await import('/src/engine/weapon/ExplosionEffect.ts');
        const configs = { rocket: fx.ROCKET_EXPLOSION, grenade: fx.GRENADE_EXPLOSION, plasma_bolt: fx.PLASMA_EXPLOSION, blade_blast: fx.BLADE_EXPLOSION };
        const effect = fx.getExplosionEffect();
        const random = Math.random;
        for (const e of explosions.split(';')) {
          const [id, x, y, z, age] = e.split(',');
          let seed = 1;
          Math.random = () => {
            seed = (seed + 0x6d2b79f5) | 0;
            let t = Math.imul(seed ^ (seed >>> 15), seed | 1);
            t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
            return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
          };
          effect.clock = () => -Number(age);
          effect.renderer.clock = effect.clock;
          effect.spawn(new Vector3(Number(x), Number(z), Number(y)), configs[id], Vector3.Up());
        }
        Math.random = random;
        const { getPointLightPool } = await import('/src/engine/weapon/PointLightPool.ts');
        if (!lights) {
          for (const exp of effect.explosions) {
            getPointLightPool().release(exp.light);
            exp.light = null;
          }
        }
        effect.clock = () => 0;
        effect.renderer.clock = effect.clock;
      }
      if (effects) {
        // Streaks, flashes, sparks, rails and lightning through SHAG's own renderers, each
        // spawned on a clock set back by its age, then held at 0. The
        // impact scatter restarts from its seed, the flash and spark seeds
        // from 1, and flashes stay where they were fired.
        const { Vector3, Color3 } = { ...(await import('/node_modules/@babylonjs/core/Maths/math.vector.js')), ...(await import('/node_modules/@babylonjs/core/Maths/math.color.js')) };
        const { getHitscanEffects } = await import('/src/engine/weapon/HitscanEffects.ts');
        const { mulberry32 } = await import('/src/shared/SpreadUtils.ts');
        const fx = getHitscanEffects();
        const tracer = fx.streakRenderer;
        const muzzle = fx.muzzleFlashRenderer;
        const sparks = fx.impactSparkRenderer;
        const rail = fx.railRenderer;
        for (const r of [tracer, muzzle, sparks, rail]) r.clear();
        rail.activationCounter = 0;
        const volt = fx.voltaicRenderer;
        volt.clear();
        volt.setRemoteStartResolver(null);
        // The demo's own view model would release a held hero beam.
        volt.stopHero = () => {};
        volt.heroActivation = 0;
        volt.remoteBatch.activationCounter = 0;
        volt.chainBatch.activationCounter = 0;
        muzzle.anchorResolver = null;
        muzzle.seedCounter = 0;
        sparks.seedCounter = 0;
        sparks.random = mulberry32(0x5eed);
        const q = (a, i) => new Vector3(a[i], a[i + 2], a[i + 1]);
        const c = (a, i) => new Color3(a[i], a[i + 1], a[i + 2]);
        for (const e of effects.split(';')) {
          const kind = e.slice(0, e.indexOf(':'));
          const parts = e.slice(e.indexOf(':') + 1).split(',');
          const n = parts.map(Number);
          const age = n[n.length - 1];
          const at = () => -age;
          if (kind === 'tracer') {
            tracer.clock = at;
            tracer.fire({ start: q(n, 0), end: q(n, 3), color: c(n, 6) });
          } else if (kind === 'muzzle') {
            muzzle.clock = at;
            muzzle.spawn({ position: q(n, 0), color: c(n, 3), style: parts[6], scale: n[7], duration: n[8] });
          } else if (kind === 'impact') {
            sparks.clock = at;
            sparks.burst({ position: q(n, 0), normal: q(n, 3), incoming: q(n, 6), color: c(n, 9), heavy: n[12] !== 0 });
          } else if (kind === 'lightning') {
            volt.clock = at;
            const mode = parts[0];
            const m = n.slice(1);
            const sample = { start: q(m, 0), end: q(m, 3), color: c(m, 6), impactKind: parts[10] };
            if (mode === 'hero') volt.updateHero(sample);
            else if (mode === 'remote') volt.updateRemote(`remote${volt.getDebugCounts().remote}`, sample, sample.end.subtract(sample.start));
            else volt.fireChainArc({ ...sample, intensity: 1.12 }, 0.14);
          } else if (kind === 'rail') {
            rail.clock = at;
            const normal = n[10] || n[11] || n[12] ? q(n, 10) : undefined;
            rail.fireRail({ start: q(n, 0), end: q(n, 3), color: c(n, 6), impactKind: parts[9], impactNormal: normal });
          }
        }
        for (const r of [tracer, muzzle, sparks, rail, volt]) r.clock = () => 0;
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
      // The first-person weapon: a view of its own on this camera, at the hip.
      const held = new Set();
      if (weapon) {
        const { WeaponViewManager } = await import('/src/engine/weapon/WeaponViewManager.ts');
        // The demo viewer keeps a view of its own: its weapon lights would
        // take this weapon's two light slots too.
        const before = new Set(scene.lights);
        const view = new WeaponViewManager();
        view.init(scene, camera);
        for (const l of before) if (/^(weaponLight|bladeLight|muzzleFlash)$/.test(l.name)) l.dispose();
        await view.loadWeapon(weapon);
        for (const m of view.currentWeapon?.meshes ?? []) held.add(m);
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
        window.__holdTrails?.();
        for (const m of scene.meshes) if (!keep.test(m.name) && !weapons.has(m) && m.isVisible) m.isVisible = false;
        // The paused demo leaves taken pickups disabled, and a pickup hides its
        // model from a camera whose cluster cannot see it, judged before this
        // camera moved (Pickup.applyModelInView).
        for (const p of FPSPickup.allPickups) if (p.options.type === 'weapon' && p.mesh && !p.mesh.isEnabled()) p.mesh.setEnabled(true);
        for (const m of weapons) if (m.getTotalVertices() > 0) m.isVisible = true;
        for (const m of held) m.isVisible = true;
      };
      scene.onBeforeCameraRenderObservable.add(place);
      for (const el of document.body.querySelectorAll('body > *')) if (!el.querySelector('canvas') && el.tagName !== 'CANVAS') el.style.visibility = 'hidden';
    }, [keepRe, players, itemTime, projectiles, explosions, effects, lights, weapon]);
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
