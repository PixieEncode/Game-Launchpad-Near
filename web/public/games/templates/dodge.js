/*
 * Template "dodge": dodge & catch. Player at the bottom moves left/right (drag /
 * arrows / A-D); hazards are telegraphed by a marker, then fall; collectibles fall
 * too. Every ~11 s a PUMP (collectible shower) or DUMP (hazard wall) wave hits.
 * The rules live in the deterministic core (sim/src/templates/dodge.rs); this file
 * only draws it. Contract: src/lib/games/TEMPLATE_API.md
 */
(function () {
  "use strict";
  if (!window.POP) return;
  var clamp = POP.util.clamp;
  var sync = POP.util.sync;
  var PLAYER = 52;

  function hexInt(h) {
    return parseInt(String(h || "#ffffff").slice(1), 16) || 0xffffff;
  }
  /** An image fitted inside w x h, keeping the texture's aspect. */
  function sprite(scene, key, x, y, w, h) {
    var img = scene.add.image(x, y, key);
    var fw = img.width || w;
    var fh = img.height || h;
    var s = Math.min(w / fw, h / fh);
    img.setDisplaySize(fw * s, fh * s);
    return img;
  }

  POP.registerTemplate("dodge", {
    titleHint: "DRAG TO MOVE",
    background: true,
    decorDirection: "down",
    groundY: 664,

    create: function (ctx) {
      var scene = ctx.scene;
      var c = ctx.config;
      ctx.drawGround();
      var floorY = 664;
      var st = (ctx._dodge = { t: 0, lean: 0, hazards: {}, items: {}, floorY: floorY, accent: hexInt(c.palette.accent) });
      st.pshadow = scene.add.ellipse(ctx.W / 2, floorY + 2, PLAYER * 0.9, 12, 0x000000, 0.3).setDepth(3.5);
      st.player = sprite(scene, ctx.tex.player, ctx.W / 2, floorY + 2, PLAYER, PLAYER).setOrigin(0.5, 1).setDepth(5);
      st.bsx = st.player.scaleX;
      st.bsy = st.player.scaleY;
      st.dust = scene.add
        .particles(0, 0, "pop_dot", {
          emitting: false,
          speedX: { min: -120, max: 120 },
          speedY: { min: -80, max: -10 },
          scale: { start: 0.8, end: 0 },
          alpha: { start: 0.6, end: 0 },
          lifespan: { min: 220, max: 400 },
          tint: [hexInt(POP.util.shade(c.palette.ground, 0.4)), 0xffffff],
        })
        .setDepth(4.6);
      st.sparkle = scene.add
        .particles(0, 0, "pop_dot", {
          emitting: false,
          speed: { min: 90, max: 260 },
          scale: { start: 1, end: 0 },
          alpha: { start: 1, end: 0 },
          lifespan: { min: 260, max: 480 },
          blendMode: "ADD",
          tint: [st.accent, 0xffffff, 0xffd23f],
        })
        .setDepth(6);
    },

    render: function (ctx, snap, events, dtMs) {
      var st = ctx._dodge;
      var scene = ctx.scene;
      var dt = dtMs / 1000;
      var p = st.player;
      var pl = snap.player;
      st.t += dt;
      var landed = {};

      for (var i = 0; i < events.length; i++) {
        var ev = events[i];
        if (ev.e === "wave") {
          var dump = ev.kind === "dump";
          ctx.popup(dump ? "DUMP!" : "PUMP!", ctx.W / 2, 200, { size: 44, color: dump ? "#ff4d4d" : "#3dff8a", kind: "big" });
          if (dump) ctx.shake(0.3);
        } else if (ev.e === "hazard_land") {
          landed[ev.id] = true;
          st.dust.explode(6, ev.x, st.floorY);
        } else if (ev.e === "collect") {
          st.sparkle.explode(12, ev.x, ev.y);
          ctx.squash(p, 1.22, 0.82, 220);
        }
      }

      // player: lean into the move
      p.x = pl.x;
      p.y = pl.y;
      var leanT = clamp(pl.v * 0.035, -16, 16);
      st.lean += (leanT - st.lean) * Math.min(1, dt * 14);
      p.setAngle(st.lean);
      st.pshadow.x = pl.x;

      sync(
        st.hazards,
        snap.ents,
        ["hazard"],
        function (e) {
          var img = sprite(scene, ctx.tex.obstacle, e.x, e.y, e.w, e.h).setDepth(3).setVisible(e.a <= 0);
          var marker = scene.add.triangle(e.x, 8, 0, 0, 32, 0, 16, 24, st.accent).setOrigin(0.5, 0).setDepth(7).setStrokeStyle(3, 0xffffff, 1);
          var shadow = scene.add.ellipse(e.x, st.floorY + 3, e.w * 1.1, 12, 0x000000, 0.2).setDepth(3.4);
          return {
            id: e.id,
            img: img,
            marker: marker,
            shadow: shadow,
            spin: (Math.random() * 2 - 1) * 90 * ctx.config.obstacles.variance,
            r: e.b,
          };
        },
        function (h, e) {
          var prox = clamp(1 - (st.floorY - e.y) / st.floorY, 0, 1);
          h.shadow.setScale(0.45 + 0.65 * prox, 0.45 + 0.65 * prox).setAlpha(0.2 + 0.35 * prox);
          if (e.a > 0) {
            var pulse = Math.abs(Math.sin(st.t * 22));
            h.marker.setAlpha(0.6 + 0.4 * pulse).setScale(1 + 0.25 * pulse);
            return;
          }
          if (h.marker) {
            h.marker.destroy();
            h.marker = null;
            h.img.setVisible(true);
          }
          h.img.y = e.y;
          h.img.angle += h.spin * dt;
        },
        function (h, id) {
          if (h.marker) h.marker.destroy();
          h.shadow.destroy();
          if (!landed[id]) return h.img.destroy();
          // landed safely: squash into the floor and fade
          var img = h.img;
          img.y = st.floorY - h.r * 0.6;
          var sx = img.scaleX;
          var sy = img.scaleY;
          img.setScale(sx * 1.5, sy * 0.5);
          scene.tweens.chain({
            targets: img,
            tweens: [
              { scaleX: sx * 0.85, scaleY: sy * 1.15, y: img.y - 26, duration: 160, ease: "Quad.easeOut" },
              { scaleX: sx * 1.2, scaleY: sy * 0.7, y: st.floorY - h.r * 0.5, alpha: 0, duration: 200, ease: "Quad.easeIn" },
            ],
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
          img.setAngle(Math.sin(st.t * 4 + img.__ph) * 14);
        },
        function (img) {
          if (img.y < st.floorY) {
            scene.tweens.add({ targets: img, scaleX: img.scaleX * 1.8, scaleY: img.scaleY * 1.8, alpha: 0, duration: 160, onComplete: function () { img.destroy(); } });
          } else img.destroy();
        }
      );

      if (snap.over && !st.dead) {
        st.dead = true;
        p.setTint(0xff7777);
        scene.tweens.add({ targets: p, scaleX: st.bsx * 1.4, scaleY: st.bsy * 0.5, duration: 160, ease: "Quad.easeOut" });
      }
    },
  });
})();
