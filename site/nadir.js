// The demo page's scene tabs and feature filters (ADR-0029). Without this
// file every scene shows, one under the other, with its own controls, and
// so does every feature. It plays the scene in view, but never one the
// viewer paused.
(() => {
  'use strict';

  const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  // The videos the viewer paused, which only the viewer plays again.
  const heldByViewer = new WeakSet();
  // The videos this script is pausing, whose pause event is not the viewer's.
  const pausing = new WeakSet();

  // Plays a video, quietly giving up where the browser will not autoplay.
  const play = (video) => {
    if (heldByViewer.has(video)) return;
    const started = video.play();
    if (started && typeof started.catch === 'function') started.catch(() => {});
  };

  const pause = (video) => {
    if (video.paused) return;
    pausing.add(video);
    video.pause();
  };

  document.querySelectorAll('video').forEach((video) => {
    video.addEventListener('pause', () => {
      if (pausing.has(video)) pausing.delete(video);
      else heldByViewer.add(video);
    });
    video.addEventListener('play', () => heldByViewer.delete(video));
  });

  // ---------- Scenes ----------
  const tabList = document.querySelector('[data-scene-tabs]');
  const scenes = [...document.querySelectorAll('[data-scene]')];
  if (tabList && scenes.length > 1) {
    const tabs = [...tabList.querySelectorAll('[role="tab"]')];
    let visible = true;

    // The scenes become the tabs' panels.
    scenes.forEach((scene) => {
      scene.setAttribute('role', 'tabpanel');
      scene.setAttribute('aria-labelledby', `tab-${scene.dataset.scene}`);
    });

    const select = (index, focus) => {
      tabs.forEach((tab, i) => {
        const on = i === index;
        tab.classList.toggle('is-active', on);
        tab.setAttribute('aria-selected', String(on));
        tab.tabIndex = on ? 0 : -1;
      });
      scenes.forEach((scene, i) => {
        const video = scene.querySelector('video');
        scene.hidden = i !== index;
        if (!video) return;
        if (i === index) {
          video.preload = 'auto';
          if (!reduceMotion && visible) play(video);
        } else {
          pause(video);
        }
      });
      if (focus) tabs[index].focus();
    };

    // The viewer chose a scene, which asks for it to play.
    const choose = (index, focus) => {
      const video = scenes[index].querySelector('video');
      if (video) heldByViewer.delete(video);
      select(index, focus);
    };

    tabs.forEach((tab, index) => {
      tab.addEventListener('click', () => choose(index, false));
      tab.addEventListener('keydown', (event) => {
        const step = { ArrowRight: 1, ArrowLeft: -1 }[event.key];
        if (event.key === 'Home' || event.key === 'End') {
          event.preventDefault();
          choose(event.key === 'Home' ? 0 : tabs.length - 1, true);
        } else if (step) {
          event.preventDefault();
          choose((index + step + tabs.length) % tabs.length, true);
        }
      });
    });

    // Plays only while the player is on screen.
    if ('IntersectionObserver' in window) {
      new IntersectionObserver((entries) => {
        visible = entries.some((entry) => entry.isIntersecting);
        const current = scenes.find((scene) => !scene.hidden);
        const video = current && current.querySelector('video');
        if (!video) return;
        if (visible && !reduceMotion) play(video);
        else pause(video);
      }).observe(tabList.closest('section') || tabList);
    }

    tabList.hidden = false;
    select(0, false);
  }
  if (reduceMotion) {
    document.querySelectorAll('video[autoplay]').forEach((video) => {
      video.removeAttribute('autoplay');
      pause(video);
    });
  }

  // ---------- Feature filters ----------
  const filters = document.querySelector('[data-feature-filters]');
  const cards = [...document.querySelectorAll('[data-features] [data-status]')];
  if (filters && cards.length) {
    const chips = [...filters.querySelectorAll('[data-status-filter]')];
    chips.forEach((chip) => chip.addEventListener('click', () => {
      const status = chip.dataset.statusFilter;
      chips.forEach((other) => {
        const on = other === chip;
        other.classList.toggle('is-active', on);
        other.setAttribute('aria-pressed', String(on));
      });
      cards.forEach((card) => {
        card.hidden = status !== 'all' && card.dataset.status !== status;
      });
    }));
    filters.hidden = false;
  }
})();
