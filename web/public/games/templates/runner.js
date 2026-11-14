/* Runner template: tap to jump over ground obstacles. See src/lib/games/TEMPLATE_API.md.
 * The rules live in the deterministic core (sim/src/templates/runner.rs); this file only draws it. */
(function () {
  "use strict";
  if (!window.POP) return;
  var clamp = POP.util.clamp;
  var sync = POP.util.sync;

  POP.registerTemplate("runner", {
    titleHint: "TAP TO JUMP",
    groundY: 610,
    sizes: { obstacle: { w: 52, h: 68 } },

    create: function (ctx) {
      var s = ctx.scene;
      ctx.drawGround();
      var st = (ctx.runner = { t: 0, obstacles: {}, items: {} });
      st.shadow = s.add.ellipse(110, ctx.groundY + 3, 38, 10, 0x000000, 0.25).setDepth(3.5);
      st.player = s.add.image(110, ctx.groundY, ctx.tex.player).setOrigin(0.5, 1).setDepth(5);
      st.player.setDisplaySize(48, (48 * ctx.size.player.h) / ctx.size.player.w);
      st.base = { x: st.player.scaleX, y: st.player.scaleY };
      ctx.trail(st.player);
      var gc = POP.util.hexToInt(POP.util.shade(ctx.palette.ground, 0.35));
      st.puff = s.add
        .particles(0, 0, "pop_dot", {
          emitting: false,
          speedX: { min: -140, max: 40 },
          speedY: { min: -70, max: -10 },
          scale: { start: 0.9, end: 0 },
          alpha: { start: 0.7, end: 0 },
          lifespan: { min: 240, max: 420 },
          tint: [gc, 0xffffff],
        })
        .setDepth(4.6);
    },

    idle: function (ctx, dtMs) {
      var st = ctx.runner;
      st.t += dtMs / 1000;
      st.player.y = ctx.groundY - Math.abs(Math.sin(st.t * 6)) * 6;
    },

    render: function (ctx, snap, events, dtMs) {
      var st = ctx.runner;
      var s = ctx.scene;
      var dt = dtMs / 1000;
      var p = st.player;
      var pl = snap.player;
      var b = st.base;
      st.t += dt;

      for (var i = 0; i < events.length; i++) {
        var ev = events[i];
        if (ev.e === "jump") {
          ctx.sfx("jump");
          s.tweens.killTweensOf(p);
          p.setScale(b.x * 0.78, b.y * 1.25);
          s.tweens.add({ targets: p, scaleX: b.x, scaleY: b.y, duration: 280, ease: "Quad.easeOut" });
          st.puff.explode(4, pl.x, ctx.groundY);
        } else if (ev.e === "land") {
          var k = clamp(ev.impact / ctx.config.physics.jumpVelocity, 0.4, 1.2);
          s.tweens.killTweensOf(p);
          p.setScale(b.x * (1 + 0.3 * k), b.y * (1 - 0.28 * k));
          s.tweens.add({ targets: p, scaleX: b.x, scaleY: b.y, duration: 220, ease: "Back.easeOut" });
          st.puff.explode(Math.round(4 + 4 * k), pl.x, ctx.groundY);
        }
      }

      var bob = pl.grounded ? Math.abs(Math.sin(st.t * 16)) * 3 : 0;
      p.x = pl.x;
      p.y = pl.y - bob;
      var tilt = pl.grounded ? Math.sin(st.t * 16) * 3 : clamp(pl.v * 0.02, -14, 16);
      p.setAngle(p.angle + (tilt - p.angle) * Math.min(1, dt * 14));
      var sh = clamp(1 - (ctx.groundY - pl.y) / 260, 0.35, 1);
      st.shadow.setScale(sh, sh).setAlpha(0.28 * sh);

      sync(
        st.obstacles,
        snap.ents,
        ["obstacle"],
        function (e) {
          var shd = s.add.ellipse(e.x, ctx.groundY + 3, e.w * 0.95, 10, 0x000000, 0.25).setDepth(3.5);
          var sp = s.add.image(e.x, ctx.groundY + 2, ctx.tex.obstacle).setOrigin(0.5, 1).setDisplaySize(e.w, e.h).setDepth(4);
          return {
            sp: sp,
            shd: shd,
            sx: sp.scaleX,
            sy: sp.scaleY,
            ph: Math.random() * 6.28,
            destroy: function () {
              sp.destroy();
              shd.destroy();
            },
          };
        },
        function (o, e) {
          o.sp.x = o.shd.x = e.x;
          o.sp.scaleY = o.sy * (1 + Math.sin(st.t * 6 + o.ph) * 0.04);
          o.sp.scaleX = o.sx * (1 - Math.sin(st.t * 6 + o.ph) * 0.03);
        }
      );

      sync(
        st.items,
        snap.ents,
        ["item"],
        function (e) {
          var sp = s.add.image(e.x, e.y, ctx.tex.collectible).setDepth(4.5);
          sp.__ph = Math.random() * 6.28;
          return sp;
        },
        function (sp, e) {
          sp.x = e.x;
          sp.y = e.y;
          sp.setScale(1 + Math.sin(st.t * 7 + sp.__ph) * 0.08);
        },
        function (sp) {
          if (sp.x > 0) s.tweens.add({ targets: sp, scale: 1.8, alpha: 0, duration: 140, onComplete: function () { sp.destroy(); } });
          else sp.destroy();
        }
      );

      if (snap.over && !st.dead) {
        st.dead = true;
        p.setTint(0xff7777);
        s.tweens.killTweensOf(p);
        p.setScale(b.x, b.y);
        s.tweens.add({ targets: p, x: pl.x - 40, angle: -100, duration: 420, ease: "Quad.easeOut" });
        s.tweens.add({ targets: p, y: pl.y - 70, duration: 200, yoyo: true, ease: "Quad.easeOut" });
      }
    },
  });
})();
