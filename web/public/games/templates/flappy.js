/* Flappy template: tap to flap through pipe pairs. See src/lib/games/TEMPLATE_API.md.
 * The rules live in the deterministic core (sim/src/templates/flappy.rs); this file only draws it. */
(function () {
  "use strict";
  if (!window.POP) return;
  var clamp = POP.util.clamp;
  var sync = POP.util.sync;

  function puffEmitter(s, tint) {
    return s.add
      .particles(0, 0, "pop_dot", {
        emitting: false,
        speed: { min: 30, max: 110 },
        angle: { min: 120, max: 200 },
        scale: { start: 0.7, end: 0 },
        alpha: { start: 0.8, end: 0 },
        lifespan: { min: 220, max: 380 },
        tint: tint,
      })
      .setDepth(4.8);
  }

  POP.registerTemplate("flappy", {
    titleHint: "TAP TO FLY",
    groundY: 660,
    sizes: { obstacle: { w: 72, h: 620, mode: "width" } },

    create: function (ctx) {
      var s = ctx.scene;
      ctx.drawGround();
      var st = (ctx.flappy = {
        px: 140,
        py: ctx.phase === "title" ? ctx.H * 0.66 : ctx.H * 0.42,
        t: 0,
        pipes: {},
        items: {},
      });
      // The core's hitboxes use a 48 px player; draw it at that size.
      st.player = s.add.image(st.px, st.py, ctx.tex.player).setDepth(5);
      st.player.setDisplaySize(48, (48 * ctx.size.player.h) / ctx.size.player.w);
      st.base = { x: st.player.scaleX, y: st.player.scaleY };
      st.puff = puffEmitter(s, [0xffffff, POP.util.hexToInt(POP.util.shade(ctx.palette.accent, 0.5))]);
      ctx.trail(st.player);
    },

    idle: function (ctx, dtMs) {
      var st = ctx.flappy;
      st.t += dtMs / 1000;
      st.player.y = st.py + Math.sin(st.t * 3) * 10;
      st.player.setAngle(Math.sin(st.t * 3 + 1) * 6);
    },

    render: function (ctx, snap, events, dtMs) {
      var st = ctx.flappy;
      var s = ctx.scene;
      var dt = dtMs / 1000;
      var p = st.player;
      var pl = snap.player;
      st.t += dt;

      for (var i = 0; i < events.length; i++) {
        if (events[i].e === "jump") {
          ctx.sfx("jump");
          s.tweens.killTweensOf(p);
          p.setScale(st.base.x * 0.78, st.base.y * 1.28);
          s.tweens.add({ targets: p, scaleX: st.base.x, scaleY: st.base.y, duration: 260, ease: "Back.easeOut" });
          st.puff.explode(5, pl.x - 14, pl.y + 10);
        }
      }

      // player: nose up while rising, ease into a dive while falling
      var jv = ctx.config.physics.jumpVelocity;
      var target = pl.v < 0 ? -24 : clamp(-24 + (pl.v / jv) * 70, -24, 70);
      var rate = target < p.angle ? 16 : 5;
      p.setAngle(p.angle + (target - p.angle) * Math.min(1, dt * rate));
      p.x = pl.x;
      p.y = pl.y;

      // pipes: a pair per core pipe; they slam shut as they enter (visual only)
      var ph = Math.max(ctx.size.obstacle.h, 640);
      sync(
        st.pipes,
        snap.ents,
        ["pipe"],
        function (e) {
          var top = s.add.image(e.x, e.a, ctx.tex.obstacle).setOrigin(0.5, 1).setFlipY(true).setDisplaySize(e.w, ph).setDepth(2);
          var bot = s.add.image(e.x, e.b, ctx.tex.obstacle).setOrigin(0.5, 0).setDisplaySize(e.w, ph).setDepth(2);
          var o = {
            top: top,
            bot: bot,
            off: 70,
            destroy: function () {
              top.destroy();
              bot.destroy();
            },
          };
          s.tweens.add({ targets: o, off: 0, duration: 420, ease: "Back.easeOut" });
          return o;
        },
        function (o, e) {
          o.top.x = o.bot.x = e.x;
          o.top.y = e.a - o.off;
          o.bot.y = e.b + o.off;
        }
      );

      // collectibles: the core bobs them; we add a gentle pulse
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
          // collected ones get a quick pop; ones that scrolled away just go
          if (sp.x > 0) {
            s.tweens.add({ targets: p, scaleX: st.base.x * 1.18, scaleY: st.base.y * 1.18, duration: 70, yoyo: true });
            s.tweens.add({ targets: sp, scale: 1.8, alpha: 0, duration: 140, onComplete: function () { sp.destroy(); } });
          } else sp.destroy();
        }
      );

      if (snap.over && !st.dead) {
        st.dead = true;
        p.setTint(0xff7777);
        s.tweens.killTweensOf(p);
        p.setScale(st.base.x, st.base.y);
        var gy = ctx.groundY - 20;
        s.tweens.add({ targets: p, y: gy, angle: 90, duration: Math.max(120, (gy - pl.y) * 1.4), ease: "Quad.easeIn" });
      }
    },
  });
})();
