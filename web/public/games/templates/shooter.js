/*
 * Template "shooter": vertical space shooter. Player near the bottom (drag /
 * arrows / A-D) auto-fires upward; enemies descend with a wobble and take
 * tuning.enemyHp hits; every ~10 s a formation, every ~15 s an elite (+2 hp, 3x).
 * The rules live in the deterministic core (sim/src/templates/shooter.rs); this
 * file only draws it. Contract: src/lib/games/TEMPLATE_API.md
 */
(function () {
  "use strict";
  if (!window.POP) return;
  var clamp = POP.util.clamp;
  var sync = POP.util.sync;
  var PLAYER = 48;

  function hexInt(h) {
    return parseInt(String(h || "#ffffff").slice(1), 16) || 0xffffff;
  }
  function sprite(scene, key, x, y, w, h) {
    var img = scene.add.image(x, y, key);
    var fw = img.width || w;
    var fh = img.height || h;
    var s = Math.min(w / fw, h / fh);
    img.setDisplaySize(fw * s, fh * s);
    return img;
  }

  POP.registerTemplate("shooter", {
    titleHint: "DRAG TO MOVE",
    background: true,
    decorDirection: "down",

    create: function (ctx) {
      var scene = ctx.scene;
      var st = (ctx._shooter = { t: 0, shots: {}, enemies: {}, items: {}, shotCount: 0, accent: hexInt(ctx.config.palette.accent) });
      st.player = sprite(scene, ctx.tex.player, ctx.W / 2, ctx.H - 110, PLAYER, PLAYER).setDepth(5);
      st.psx = st.player.scaleX;
      st.psy = st.player.scaleY;
      ctx.trail(st.player);
      st.muzzle = scene.add.image(ctx.W / 2, ctx.H - 134, "pop_dot").setDepth(5.5).setBlendMode("ADD").setTint(0xffffff).setAlpha(0);
      st.kill = scene.add
        .particles(0, 0, "pop_dot", {
          emitting: false,
          speed: { min: 80, max: 300 },
          scale: { start: 1.2, end: 0 },
          alpha: { start: 1, end: 0 },
          lifespan: { min: 250, max: 520 },
          blendMode: "ADD",
          tint: [st.accent, 0xffffff, 0xffd23f],
        })
        .setDepth(6);
    },

    render: function (ctx, snap, events, dtMs) {
      var st = ctx._shooter;
      var scene = ctx.scene;
      var dt = dtMs / 1000;
      var p = st.player;
      var pl = snap.player;
      st.t += dt;
      var killed = {};

      for (var i = 0; i < events.length; i++) {
        var ev = events[i];
        if (ev.e === "shoot") {
          st.muzzle.setAlpha(1).setScale(1.6 + Math.random() * 0.6);
          if (st.shotCount++ % 3 === 0) ctx.sfx("shoot");
        } else if (ev.e === "hit") {
          ctx.vfx("shoot", ev.x, ev.y);
          var hit = st.enemies[ev.id];
          if (hit) {
            hit.img.setTintFill(0xffffff);
            hit.flash = 0.06;
            hit.img.setScale(hit.sx * 1.15, hit.sy * 0.88);
          }
        } else if (ev.e === "kill") {
          killed[ev.id] = true;
          st.kill.explode(ev.elite ? 30 : 14, ev.x, ev.y);
          ctx.hitstop(ev.elite ? 80 : 40);
          if (ev.elite) {
            ctx.shake(0.5);
            ctx.popup("ELITE x3", ev.x, ev.y - 20, { size: 26, color: ctx.palette.accent, kind: "meme" });
          }
        } else if (ev.e === "wave") {
          ctx.popup(ev.kind === "v" ? "INCOMING V" : "WALL INCOMING", ctx.W / 2, 170, { size: 30, color: ctx.palette.accent, kind: "big" });
        }
      }

      // player: bank into the move
      p.x = pl.x;
      p.y = pl.y;
      var bank = clamp(pl.v * 0.03, -18, 18);
      p.setAngle(p.angle + (bank - p.angle) * Math.min(1, dt * 12));
      if (st.muzzle.alpha > 0) {
        st.muzzle.setAlpha(Math.max(0, st.muzzle.alpha - dt * 14));
        st.muzzle.x = pl.x;
        st.muzzle.y = pl.y - PLAYER / 2 - 4;
      }

      sync(
        st.shots,
        snap.ents,
        ["shot"],
        function (e) {
          var img = ctx.tex.projectile
            ? sprite(scene, ctx.tex.projectile, e.x, e.y, 12, 20)
            : scene.add.rectangle(e.x, e.y, 6, 18, st.accent).setStrokeStyle(2, 0xffffff, 0.6);
          img.setDepth(4);
          var glow = scene.add.image(e.x, e.y + 14, "pop_dot").setDepth(3.9).setBlendMode("ADD").setTint(st.accent).setAlpha(0.7).setScale(1.1, 2.6);
          return {
            img: img,
            glow: glow,
            destroy: function () {
              img.destroy();
              glow.destroy();
            },
          };
        },
        function (o, e) {
          o.img.x = o.glow.x = e.x;
          o.img.y = e.y;
          o.glow.y = e.y + 14;
        }
      );

      sync(
        st.enemies,
        snap.ents,
        ["enemy", "elite"],
        function (e) {
          var elite = e.k === "elite";
          var img = sprite(scene, ctx.tex.obstacle, e.x, e.y, e.w, e.h).setDepth(3);
          if (elite) img.setTint(st.accent);
          var sx = img.scaleX;
          var sy = img.scaleY;
          img.setScale(sx * 0.4, sy * 0.4);
          scene.tweens.add({ targets: img, scaleX: sx, scaleY: sy, duration: 260, ease: "Back.easeOut" });
          return { img: img, sx: sx, sy: sy, elite: elite, flash: 0, ph: Math.random() * 6.28 };
        },
        function (o, e) {
          o.img.x = e.x;
          o.img.y = e.y;
          if (o.elite) o.img.angle = Math.sin(st.t * 3 + o.ph) * 10;
          if (o.flash > 0) {
            o.flash -= dt;
            if (o.flash <= 0) {
              if (o.elite) o.img.setTint(st.accent);
              else o.img.clearTint();
              o.img.setScale(o.sx, o.sy);
            }
          }
        },
        function (o, id) {
          if (!killed[id]) return o.img.destroy();
          var img = o.img;
          img.setTintFill(0xffffff);
          scene.tweens.add({
            targets: img,
            angle: img.angle + 280,
            scaleX: o.sx * 1.7,
            scaleY: o.sy * 1.7,
            alpha: 0,
            duration: 220,
            ease: "Quad.easeOut",
            onComplete: function () {
              img.destroy();
            },
          });
        }
      );

      sync(
        st.items,
        snap.ents,
        ["item"],
        function (e) {
          var img = sprite(scene, ctx.tex.collectible, e.x, e.y, 32, 32).setDepth(3.5);
          img.__ph = Math.random() * 6.28;
          return img;
        },
        function (img, e) {
          img.x = e.x;
          img.y = e.y;
          img.setAngle(Math.sin(st.t * 4 + img.__ph) * 12);
        }
      );

      if (snap.over && !st.dead) {
        st.dead = true;
        scene.tweens.add({ targets: p, angle: p.angle + 200, alpha: 0, scaleX: st.psx * 0.3, scaleY: st.psy * 0.3, duration: 450, ease: "Quad.easeOut" });
      }
    },
  });
})();
