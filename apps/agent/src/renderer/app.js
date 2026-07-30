    import { invoke } from '@tauri-apps/api/core';
    import { listen } from '@tauri-apps/api/event';
    import { check as checkAppUpdate } from '@tauri-apps/plugin-updater';
    import { relaunch } from '@tauri-apps/plugin-process';
    import { jsPDF } from 'jspdf';
    import { getFriendlyAuthError } from './auth-service.js';

    document.documentElement.removeAttribute('native-scrollbar');

    const SVG_STROKE = 'fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"';

    function renderIcon(name, size = 16, spin = false) {
      const cls = `icon-svg${spin ? ' icon-spin' : ''}`;
      const safeSize = Math.round(toFiniteNumber(size, { min: 8, max: 64, fallback: 16 }));
      const s = `width="${safeSize}" height="${safeSize}"`;
      const icons = {
        flame: `<svg class="${cls}" ${s} viewBox="0 0 24 24" ${SVG_STROKE}><path d="M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.5-1.5-3-2-5-1.5 1-2.5 3-2.5 5a4 4 0 1 0 8 0c0-2-.5-4-2-5-.5 2-2 3.5-2 5a2.5 2.5 0 0 0 2.5 2.5z"/></svg>`,
        hourglass: `<svg class="${cls}" ${s} viewBox="0 0 24 24" ${SVG_STROKE}><path d="M5 22h14"/><path d="M5 2h14"/><path d="M17 22v-4.172a2 2 0 0 0-.586-1.414L12 12l-4.414 4.414A2 2 0 0 0 7 17.828V22"/><path d="M7 2v4.172a2 2 0 0 0 .586 1.414L12 12l4.414-4.414A2 2 0 0 0 17 6.172V2"/></svg>`,
        refresh: `<svg class="${cls}" ${s} viewBox="0 0 24 24" ${SVG_STROKE}><path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/></svg>`,
        loader: `<svg class="${cls}" ${s} viewBox="0 0 24 24" ${SVG_STROKE}><path d="M21 12a9 9 0 1 1-6.219-8.56"/></svg>`,
        logout: `<svg class="${cls}" ${s} viewBox="0 0 24 24" ${SVG_STROKE}><path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" y1="12" x2="9" y2="12"/></svg>`,
        check: `<svg class="${cls}" ${s} viewBox="0 0 24 24" ${SVG_STROKE}><polyline points="20 6 9 17 4 12"/></svg>`,
        x: `<svg class="${cls}" ${s} viewBox="0 0 24 24" ${SVG_STROKE}><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>`,
        smartphone: `<svg class="${cls}" ${s} viewBox="0 0 24 24" ${SVG_STROKE}><rect x="6" y="2" width="12" height="20" rx="2"/><line x1="12" y1="18" x2="12.01" y2="18"/></svg>`,
        target: `<svg class="${cls}" ${s} viewBox="0 0 24 24" ${SVG_STROKE}><circle cx="12" cy="12" r="10"/><circle cx="12" cy="12" r="6"/><circle cx="12" cy="12" r="2"/></svg>`,
        chevronRight: `<svg class="${cls}" ${s} viewBox="0 0 24 24" ${SVG_STROKE}><polyline points="9 18 15 12 9 6"/></svg>`,
      };
      return icons[name] || '';
    }

    let isMonitoring = false;
    let isPaused = false;
    let monitoringInterval = null;
    let captureIntervalMs = 60000;
    let captureRunId = 0;
    let monitoringActionId = 0;
    let currentUser = null;
    let authSession = null;
    let currentAccountId = null;
    let accountEpoch = 0;
    let currentEntitlements = null;
    let accountAuthPollId = 0;
    const providerAuthPollIds = { jira: 0, linear: 0 };
    const INDIVIDUAL_PLANS = new Set(['individual', 'individual_pro']);
    const TEAM_PLANS = new Set(['team', 'teams_simple', 'teams_pro', 'enterprise']);

    function guard(epoch, accountId) {
      return epoch === accountEpoch && accountId === currentAccountId;
    }

    function normalizedPlan(plan) {
      return String(plan || '').trim().toLowerCase();
    }

    function isIndividualPlan(plan) {
      return INDIVIDUAL_PLANS.has(normalizedPlan(plan));
    }

    function isTeamPlan(plan) {
      return TEAM_PLANS.has(normalizedPlan(plan));
    }

    function normalizeAccountSession(source, fallbackProvider = 'cloud') {
      if (!source) return null;

      const rawUser = source.user || source;
      const provider = String(source.provider || rawUser.provider || fallbackProvider).toLowerCase();
      if (provider === 'jira' || provider === 'linear') return null;

      const id = rawUser.id || rawUser.user_id || source.user_id || '';
      const email = rawUser.email || source.email || '';
      if (!id && !email) return null;

      const displayName = rawUser.display_name || rawUser.name || email || 'User';
      return {
        provider,
        user: {
          id,
          email,
          display_name: displayName,
          name: displayName,
          avatar_url: rawUser.avatar_url || null,
          provider,
        },
      };
    }

    function normalizeEntitlements(entitlements) {
      if (!entitlements) return null;
      return {
        plan: entitlements.plan ?? null,
        status: entitlements.status ?? 'free',
        team_ids: entitlements.team_ids ?? entitlements.teamIds ?? [],
        active_team_id: entitlements.active_team_id ?? entitlements.activeTeamId ?? null,
        can_sync: Boolean(entitlements.can_sync ?? entitlements.canSync),
        can_cloud_ai: Boolean(entitlements.can_cloud_ai ?? entitlements.canCloudAi),
        can_integrations: Boolean(entitlements.can_integrations ?? entitlements.canIntegrations),
      };
    }

    function needsPersonalWorkspace(entitlements) {
      return entitlements?.status === 'active'
        && isIndividualPlan(entitlements.plan)
        && !entitlements.active_team_id;
    }

    async function refreshAccountEntitlements() {
      let entitlements = normalizeEntitlements(await invoke('refresh_entitlements'));
      if (needsPersonalWorkspace(entitlements)) {
        await invoke('ensure_personal_team');
        entitlements = normalizeEntitlements(await invoke('refresh_entitlements'));
      }
      return persistEntitlements(entitlements);
    }

    async function ensureRestoredPersonalWorkspace() {
      if (!needsPersonalWorkspace(currentEntitlements)) return;
      await invoke('ensure_personal_team');
      await persistEntitlements(await invoke('refresh_entitlements'));
    }

    function safeUnlisten(unlisten) {
      if (typeof unlisten !== 'function') return;
      try {
        unlisten();
      } catch (error) {
        console.warn('[Events] Failed to remove listener:', error);
      }
    }

    function setButtonLoading(button, label) {
      const spinner = document.createElement('span');
      spinner.className = 'loading';
      button.replaceChildren(spinner, document.createTextNode(` ${String(label || '')}`));
    }

    function isPaidActive() {
      return Boolean(
        authSession &&
        currentAccountId &&
        currentEntitlements &&
        currentEntitlements.status === 'active' &&
        (currentEntitlements.can_sync || currentEntitlements.can_cloud_ai || currentEntitlements.can_integrations)
      );
    }

    function requirePaidFeature(feature, message) {
      const allowed = currentEntitlements && (
        feature === 'sync' ? currentEntitlements.can_sync :
        feature === 'cloud_ai' ? currentEntitlements.can_cloud_ai :
        feature === 'integrations' ? currentEntitlements.can_integrations :
        isPaidActive()
      );
      if (allowed) return true;
      showToast(message || 'This feature requires an Individual or Team license.', 'error', 4500);
      if (authSession) {
        switchTab('tabProfile');
        document.getElementById('profileLicenseCode')?.focus();
      } else {
        showCloudActivationPanel();
      }
      return false;
    }

    function setLicenseActivationStatus(message, isError = false) {
      const el = document.getElementById('licenseActivationStatus');
      if (!el) return;
      el.textContent = message || '';
      el.style.color = isError ? 'hsl(var(--destructive))' : 'hsl(var(--muted-foreground))';
    }

    function updateLicenseActivationCard() {
      const card = document.getElementById('licenseActivationCard');
      if (!card) return;
      card.classList.toggle('u-hidden', !(authSession && !isPaidActive()));
      if (!authSession || isPaidActive()) {
        setLicenseActivationStatus('');
      }
    }

    function handleActivateCloudClick() {
      if (authSession && !isPaidActive()) {
        switchTab('tabProfile');
        document.getElementById('profileLicenseCode')?.focus();
        return;
      }
      showCloudActivationPanel();
    }

    async function handleClaimLicense() {
      const code = document.getElementById('profileLicenseCode')?.value?.trim();
      if (!code) {
        setLicenseActivationStatus('Enter your license code (FS-XXXX-XXXX).', true);
        return;
      }

      const btn = document.getElementById('claimLicenseBtn');
      btn.disabled = true;
      setLicenseActivationStatus('Activating license...');

      try {
        await invoke('claim_license_code', { code });

        await refreshAccountEntitlements();

        if (!isPaidActive()) {
          throw new Error('License could not be activated. Check your code and try again.');
        }

        setLicenseActivationStatus('');
        document.getElementById('profileLicenseCode').value = '';
        await loadUserTeams();
        await restoreLinkedProviders();
        showToast('License activated', 'success');
      } catch (error) {
        const message = error?.message || 'Could not activate license.';
        setLicenseActivationStatus(message, true);
        showToast(message, 'error', 4500);
      } finally {
        btn.disabled = false;
      }
    }

    function updatePlanBadge() {
      const badge = document.getElementById('loginBadge');
      const activateBtn = document.getElementById('activateCloudBtn');
      const logoutBtn = document.getElementById('logoutBtn');
      const navCloud = document.getElementById('navCloudInsights');
      const integrationsCard = document.getElementById('integrationsCard');
      const integrationsProviderRow = document.getElementById('integrationsProviderRow');
      const upgradeHint = document.getElementById('integrationsUpgradeHint');
      const teamCodeGroup = document.getElementById('teamCodeGroup');
      const todayIntegrationControls = document.getElementById('todayIntegrationControls');
      const manualTask = document.getElementById('manualTask');
      const syncToJiraWrapper = document.getElementById('syncToJiraWrapper');
      const licenseCard = document.getElementById('licenseActivationCard');
      const canIntegrate = Boolean(currentEntitlements?.can_integrations);
      const selectedTaskSource = document.getElementById('jiraSelect')?.selectedOptions?.[0]?.dataset.source;

      if (!badge) return;

      badge.classList.remove('plan-badge-free', 'plan-badge-individual', 'plan-badge-team', 'badge-success');

      if (todayIntegrationControls) todayIntegrationControls.classList.toggle('u-hidden', !canIntegrate);
      if (manualTask) manualTask.classList.toggle('u-hidden', canIntegrate);
      if (syncToJiraWrapper) syncToJiraWrapper.classList.toggle('u-hidden', !(canIntegrate && selectedTaskSource === 'jira'));

      if (!isPaidActive()) {
        if (authSession) {
          badge.textContent = 'Cloud account';
          badge.classList.add('plan-badge-free');
          if (activateBtn) {
            activateBtn.classList.remove('u-hidden');
            activateBtn.textContent = 'Activate license';
          }
          if (logoutBtn) logoutBtn.classList.remove('u-hidden');
        } else {
          badge.textContent = 'Free (Local)';
          badge.classList.add('plan-badge-free');
          if (activateBtn) {
            activateBtn.classList.remove('u-hidden');
            activateBtn.textContent = 'Activate cloud';
          }
          if (logoutBtn) logoutBtn.classList.add('u-hidden');
        }
        if (navCloud) {
          navCloud.classList.remove('u-hidden');
          navCloud.classList.add('nav-pro-locked');
        }
        if (integrationsCard) integrationsCard.classList.add('paid-feature-locked');
        if (integrationsProviderRow) integrationsProviderRow.classList.add('u-hidden');
        if (upgradeHint) upgradeHint.classList.remove('u-hidden');
        if (teamCodeGroup) teamCodeGroup.classList.add('u-hidden');
        updateLicenseActivationCard();
        updateCoachLockState();
        return;
      }

      if (licenseCard) licenseCard.classList.add('u-hidden');

      const plan = normalizedPlan(currentEntitlements.plan) || 'paid';
      badge.textContent = isIndividualPlan(plan)
        ? 'Individual'
        : isTeamPlan(plan)
          ? (plan === 'enterprise' ? 'Enterprise' : 'Team')
          : 'Licensed';
      badge.classList.add(isIndividualPlan(plan) ? 'plan-badge-individual' : 'plan-badge-team');
      if (activateBtn) activateBtn.classList.add('u-hidden');
      if (logoutBtn) logoutBtn.classList.remove('u-hidden');
      if (integrationsCard) integrationsCard.classList.remove('paid-feature-locked');
      if (integrationsProviderRow) integrationsProviderRow.classList.toggle('u-hidden', !canIntegrate);
      if (upgradeHint) upgradeHint.classList.toggle('u-hidden', canIntegrate);
      if (teamCodeGroup) teamCodeGroup.classList.toggle('u-hidden', !isTeamPlan(plan));

      if (navCloud) {
        navCloud.classList.remove('u-hidden');
        navCloud.classList.toggle('nav-pro-locked', !currentEntitlements.can_cloud_ai);
      }
      updateCoachLockState();
    }

    async function persistEntitlements(entitlements) {
      currentEntitlements = normalizeEntitlements(entitlements);
      updatePlanBadge();
      return currentEntitlements;
    }

    async function loadEntitlementsFromBackend() {
      try {
        const entitlements = await invoke('get_entitlements');
        currentEntitlements = normalizeEntitlements(entitlements);
        updatePlanBadge();
        return currentEntitlements;
      } catch (e) {
        console.warn('[Entitlements] load failed:', e);
        currentEntitlements = null;
        updatePlanBadge();
        return null;
      }
    }

    function resetLoginSplashCopy() {
      const title = document.getElementById('loginTitle');
      const sub = document.getElementById('loginSubtitle');
      if (title) title.textContent = 'Welcome to Flowmates.';
      if (sub) sub.textContent = 'Activity measured on this Mac.';
    }

    function showCloudActivationPanel(mode = 'cloud') {
      const title = document.getElementById('loginTitle');
      const sub = document.getElementById('loginSubtitle');
      if (mode === 'cloud') {
        if (title) title.textContent = 'Activate cloud features';
        if (sub) sub.textContent = 'Sign in, then activate your license in Profile.';
      } else {
        resetLoginSplashCopy();
      }
      document.getElementById('loginScreen')?.classList.add('visible');
    }

    function hideCloudActivationPanel() {
      invoke('cancel_auth').catch(() => {});
      accountAuthPollId += 1;
      document.getElementById('loginScreen')?.classList.remove('visible');
      setWorkerLoginError('');
    }

    function showFreeProfileState() {
      currentUser = null;
      authSession = null;
      document.getElementById('userName').textContent = 'Guest';
      document.getElementById('userEmail').textContent = 'Free local mode';
      const avatar = document.getElementById('userAvatar');
      if (avatar) {
        avatar.classList.add('u-hidden');
        avatar.src = '';
      }
      updatePlanBadge();
    }

    function reportRuntimeError(error) {
      console.error('[Flowmates] Runtime error:', error);
      const message = error?.message || String(error || 'Unexpected app error');
      const loginErrorEl = document.getElementById('workerLoginError');
      if (loginErrorEl && document.getElementById('loginScreen')?.classList.contains('visible')) {
        loginErrorEl.textContent = message;
        loginErrorEl.classList.remove('u-hidden');
      }
      if (typeof showToast === 'function') {
        showToast(message, 'error', 6000);
      }
    }

    window.addEventListener('error', (event) => reportRuntimeError(event.error || event.message));
    window.addEventListener('unhandledrejection', (event) => reportRuntimeError(event.reason));

    // ===== AUTH FUNCTIONS =====
    async function restoreCloudSession({ refreshEntitlements = false } = {}) {
      if (refreshEntitlements) {
        try {
          await refreshAccountEntitlements();
        } catch (error) {
          console.warn('[Entitlements] refresh failed during login:', error);
          await loadEntitlementsFromBackend();
        }
      } else {
        await loadEntitlementsFromBackend();
      }

      try {
        const session = await invoke('get_auth_session');
        const identity = normalizeAccountSession(session, 'google');
        if (identity) {
          authSession = identity;
          currentUser = identity.user;
          accountEpoch += 1;
          currentAccountId = identity.user.id || null;
          await ensureRestoredPersonalWorkspace().catch((error) => {
            console.warn('[Entitlements] Could not ensure restored personal workspace:', error);
          });
          updateProfileHeader();
          return true;
        }
      } catch (e) {
        console.log("[Auth] No OAuth session:", e);
      }

      try {
        const workerSession = await invoke('get_current_user');
        const identity = normalizeAccountSession(workerSession, 'cloud');
        if (identity) {
          authSession = identity;
          currentUser = identity.user;
          accountEpoch += 1;
          currentAccountId = identity.user.id || null;
          await ensureRestoredPersonalWorkspace().catch((error) => {
            console.warn('[Entitlements] Could not ensure restored personal workspace:', error);
          });
          updateProfileHeader();
          return true;
        }
      } catch (e) {
        console.log("[Auth] No cloud session:", e);
      }

      showFreeProfileState();
      return false;
    }

    function updateProfileHeader() {
      if (!currentUser) {
        showFreeProfileState();
        return;
      }
      document.getElementById('userName').textContent = currentUser.display_name || currentUser.name || currentUser.email || 'User';
      document.getElementById('userEmail').textContent = currentUser.email || 'Cloud features active';
      const avatar = document.getElementById('userAvatar');
      const avatarUrl = safeImageUrl(currentUser.avatar_url);
      if (avatarUrl) {
        avatar.src = avatarUrl;
        avatar.classList.remove('u-hidden');
      } else {
        avatar.classList.add('u-hidden');
        avatar.src = '';
      }
      updatePlanBadge();
    }

    function showMainApp() {
      document.getElementById('loginScreen')?.classList.remove('visible');
      document.getElementById('mainApp').classList.remove('u-hidden');
      updateProfileHeader();

      if (isPaidActive()) {
        loadUserTeams();
      }
      if (currentEntitlements?.can_integrations) {
        restoreLinkedProviders();
      }

      updateTodayGreeting();
    }

    async function handleWorkerLogin(e) {
      e.preventDefault();
      const email = document.getElementById('workerEmail').value.trim();
      const password = document.getElementById('workerPassword').value;
      const btn = document.getElementById('workerLoginBtn');

      if (!email || !password) {
        setWorkerLoginError('Enter your email and password.');
        return;
      }

      setWorkerLoginError('');
      btn.disabled = true;
      setButtonLoading(btn, 'Signing in...');

      let backendSessionCreated = false;
      let authenticationCommitted = false;
      try {
        const session = await invoke('login_with_password', { email, password });
        backendSessionCreated = true;
        const identity = normalizeAccountSession(session, 'cloud');
        if (!identity) throw new Error('The login response did not include an account identity.');

        await refreshAccountEntitlements();
        authSession = identity;
        currentUser = identity.user;
        accountEpoch += 1;
        currentAccountId = identity.user.id || null;
        authenticationCommitted = true;
        document.getElementById('workerPassword').value = '';
        hideCloudActivationPanel();
        showMainApp();
        await initMainApp();
        await promptOnboardingIfNeeded();
        showToast(
          isPaidActive()
            ? 'Cloud features activated'
            : 'Signed in — activate your license in Profile',
          'success',
        );
      } catch (error) {
        if (!authenticationCommitted && backendSessionCreated) {
          await invoke('logout').catch((cleanupError) => console.error('[Auth] Backend cleanup failed:', cleanupError));
        }
        if (!authenticationCommitted) {
          authSession = null;
          currentUser = null;
          currentEntitlements = null;
          updatePlanBadge();
        }
        const message = error?.message || getFriendlyAuthError(error);
        setWorkerLoginError(message);
        showToast(message, 'error', 4500);
      } finally {
        document.getElementById('workerPassword').value = '';
        btn.disabled = false;
        btn.textContent = 'Sign in';
      }
    }

    let dailyGoalHours = 6;

    function getDailyGoalHours() {
      return dailyGoalHours;
    }

    function syncDailyGoalSelect() {
      const sel = document.getElementById('dailyGoalSelect');
      if (!sel) return;
      const val = String(dailyGoalHours);
      if (![...sel.options].some((o) => o.value === val)) {
        const opt = document.createElement('option');
        opt.value = val;
        opt.textContent = dailyGoalHours === 0 ? 'No goal' : `${dailyGoalHours} hours`;
        sel.appendChild(opt);
      }
      sel.value = val;
    }

    async function setDailyGoalHours(hours) {
      dailyGoalHours = toFiniteNumber(hours, { min: 0, max: 24, fallback: 6 });
      try {
        await invoke('update_config', { patch: { dailyGoalHours } });
      } catch (e) {
        console.error('Failed to save daily goal:', e);
        showToast('Could not save daily goal', 'error');
        return;
      }
      syncDailyGoalSelect();
      updateGoalUI();
      refreshTodayView();
    }

    function updateTodayGreeting() {
      const el = document.getElementById('todayGreeting');
      if (!el) return;
      const h = new Date().getHours();
      let period;
      if (h < 12) period = 'morning';
      else if (h < 18) period = 'afternoon';
      else period = 'night';
      const name = currentUser?.display_name || currentUser?.name || userPreferences?.displayName || 'there';
      const first = String(name).split(' ')[0];
      el.textContent = `Good ${period}, ${first}!`;
    }

    function formatTimerDisplay(totalSeconds) {
      const h = Math.floor(totalSeconds / 3600);
      const m = Math.floor((totalSeconds % 3600) / 60);
      const s = totalSeconds % 60;
      if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
      return `${m}:${String(s).padStart(2, '0')}`;
    }

    function computeStreak(week) {
      if (!week?.days) return 0;
      let streak = 0;
      const todayIdx = week.days.findIndex(d => d.is_today);
      for (let i = todayIdx; i >= 0; i--) {
        if (week.days[i].has_activity) streak++;
        else break;
      }
      return streak;
    }

    let todayTotalSeconds = 0;
    let sessionElapsedMs = 0;
    let pendingActivityMs = 0;
    let trackingClockLastTickMs = null;
    let sessionTimerInterval = null;
    let todayRefreshInterval = null;
    let todayRefreshInFlight = null;
    const TIMER_RING_CIRC = 2 * Math.PI * 82;
    const TRACKING_SLEEP_GAP_MS = 15000;

    function updateGoalUI() {
      const hours = getDailyGoalHours();
      const label = document.getElementById('timerGoalLabel');
      if (label) label.textContent = hours > 0 ? `Daily goal ${hours}h` : 'No goal target';
      syncDailyGoalSelect();
    }

    function updateTimerRing(progress) {
      const ring = document.getElementById('timerProgressRing');
      if (!ring) return;
      const p = Math.min(Math.max(progress, 0), 1);
      ring.setAttribute('stroke-dashoffset', String(TIMER_RING_CIRC * (1 - p)));
    }

    function getDisplayTotalSeconds() {
      return todayTotalSeconds + Math.floor(sessionElapsedMs / 1000);
    }

    function resetTrackingClock({ clearPending = false } = {}) {
      trackingClockLastTickMs = Date.now();
      if (clearPending) pendingActivityMs = 0;
    }

    function advanceTrackingClock() {
      const now = Date.now();
      if (!isMonitoring || trackingClockLastTickMs == null) {
        trackingClockLastTickMs = now;
        return;
      }

      const elapsedMs = now - trackingClockLastTickMs;
      trackingClockLastTickMs = now;
      if (elapsedMs <= 0) return;

      if (elapsedMs > TRACKING_SLEEP_GAP_MS) {
        console.log(`[Tracking] Ignoring ${Math.round(elapsedMs / 1000)}s timer gap (sleep or suspended WebView)`);
        return;
      }

      sessionElapsedMs += elapsedMs;
      pendingActivityMs += elapsedMs;
    }

    function pendingActivitySeconds() {
      advanceTrackingClock();
      return Math.floor(pendingActivityMs / 1000);
    }

    function consumePendingActivity(seconds) {
      pendingActivityMs = Math.max(0, pendingActivityMs - Math.max(0, seconds) * 1000);
    }

    function updateTimerControlsVisibility() {
      const stopBtn = document.getElementById('stopTimerBtn');
      if (stopBtn) {
        if (isMonitoring || isPaused) {
          stopBtn.style.display = 'flex';
          stopBtn.classList.remove('u-hidden');
        } else {
          stopBtn.classList.add('u-hidden');
        }
      }
    }

    function updatePlayButtonState() {
      const btn = document.getElementById('playTimerBtn');
      const playIcon = document.getElementById('playIcon');
      const pauseIcon = document.getElementById('pauseIcon');
      if (!btn) return;
      if (isMonitoring) {
        btn.classList.add('active');
        btn.title = 'Pause tracking';
        if (playIcon) playIcon.classList.add('u-hidden');
        if (pauseIcon) pauseIcon.classList.remove('u-hidden');
      } else {
        btn.classList.remove('active');
        btn.title = isPaused ? 'Resume tracking' : 'Start tracking';
        if (playIcon) playIcon.classList.remove('u-hidden');
        if (pauseIcon) pauseIcon.classList.add('u-hidden');
      }
      updateTimerControlsVisibility();
    }

    function updateTimerDisplay() {
      const display = document.getElementById('timerDisplay');
      if (!display) return;
      const total = getDisplayTotalSeconds();
      display.textContent = formatTimerDisplay(total);
      const goalSec = getDailyGoalHours() * 3600;
      updateTimerRing(goalSec > 0 ? total / goalSec : 0);
    }

    function commitSessionTime() {
      if (sessionTimerInterval) {
        clearInterval(sessionTimerInterval);
        sessionTimerInterval = null;
      }
      todayTotalSeconds += Math.floor(sessionElapsedMs / 1000);
      sessionElapsedMs = 0;
      trackingClockLastTickMs = null;
      updateTimerDisplay();
    }

    function resumeSessionTimer() {
      if (sessionTimerInterval) clearInterval(sessionTimerInterval);
      resetTrackingClock();
      sessionTimerInterval = setInterval(() => {
        advanceTrackingClock();
        updateTimerDisplay();
      }, 1000);
    }

    function startSessionTimer() {
      sessionElapsedMs = 0;
      lastGoodSnapshot = null;
      resetTrackingClock({ clearPending: true });
      resumeSessionTimer();
    }

    async function pauseMonitoring() {
      const actionId = ++monitoringActionId;
      advanceTrackingClock();
      isMonitoring = false;
      isPaused = true;
      captureRunId += 1;
      clearCaptureRetry();
      commitSessionTime();
      if (monitoringInterval) {
        clearInterval(monitoringInterval);
        monitoringInterval = null;
      }
      document.getElementById('statStatus').textContent = 'Paused';
      document.getElementById('statStatus').style.color = 'hsl(var(--summary-orange))';
      const badge = document.getElementById('monitoringBadge');
      if (badge) { badge.textContent = 'Paused'; badge.className = 'badge badge-default'; }
      updatePlayButtonState();
      try {
        await flushPendingActivity();
      } catch (e) {
        console.error('Failed to save tracked time while pausing:', e);
      }
      if (monitoringActionId !== actionId) return;
      showToast('Tracking paused — time kept for today', 'success');
    }

    async function stopMonitoring() {
      const actionId = ++monitoringActionId;
      advanceTrackingClock();
      isMonitoring = false;
      isPaused = false;
      captureRunId += 1;
      clearCaptureRetry();
      commitSessionTime();
      if (monitoringInterval) {
        clearInterval(monitoringInterval);
        monitoringInterval = null;
      }
      document.getElementById('statStatus').textContent = 'Off';
      document.getElementById('statStatus').style.color = '';
      const badge = document.getElementById('monitoringBadge');
      if (badge) { badge.textContent = 'Off'; badge.className = 'badge badge-default'; }
      updatePlayButtonState();
      try {
        await flushPendingActivity({ discardRemainder: true });
      } catch (e) {
        console.error('Failed to save remaining tracked time:', e);
      }
      if (monitoringActionId !== actionId) return;
      try {
        await invoke('stop_server');
        ollamaConfirmedOnline = false;
        showToast('Tracking stopped', 'success');
      } catch (e) {
        console.error('Failed to stop server:', e);
      }
      await refreshTodayView();
    }

    async function resumeMonitoring() {
      const actionId = ++monitoringActionId;
      const btn = document.getElementById('playTimerBtn');
      btn.disabled = true;
      btn.classList.add('loading-state');
      try {
        const status = await invoke('check_local_server');
        if (monitoringActionId !== actionId) return;
        if (!status.online) {
          isPaused = false;
          await startMonitoringFull({ resume: true });
          return;
        }
        isMonitoring = true;
        isPaused = false;
        captureRunId += 1;
        const runId = captureRunId;
        document.getElementById('statStatus').textContent = 'On';
        document.getElementById('statStatus').style.color = 'hsl(142.1 76.2% 36.3%)';
        const badge = document.getElementById('monitoringBadge');
        if (badge) { badge.textContent = 'On'; badge.className = 'badge badge-success'; }
        resumeSessionTimer();
        updatePlayButtonState();
        scheduleMonitoringCaptures(runId);
        showToast('Tracking resumed', 'success');
      } catch (e) {
        console.error('Resume failed:', e);
        showToast('Error resuming: ' + e, 'error');
      } finally {
        btn.disabled = false;
        btn.classList.remove('loading-state');
        updatePlayButtonState();
      }
    }

    async function refreshTodayView() {
      if (todayRefreshInFlight) return todayRefreshInFlight;

      todayRefreshInFlight = (async () => {
        updateTodayGreeting();
        updateGoalUI();
        try {
          const week = await invoke('get_week_summary').catch(() => null);
          const streak = computeStreak(week);
          const streakEl = document.getElementById('streakText');
          if (streakEl) streakEl.textContent = `${streak} day${streak === 1 ? '' : 's'} Streak`;
          if (!isMonitoring && !isPaused) {
            const history = await invoke('get_today_history');
            todayTotalSeconds = history.total_seconds || 0;
          } else if (isPaused) {
            const history = await invoke('get_today_history');
            todayTotalSeconds = Math.max(todayTotalSeconds, history.total_seconds || 0);
          }
        } catch (_) { /* agent may not be init yet */ }
        updateTimerDisplay();
        updatePlayButtonState();
      })();

      try {
        return await todayRefreshInFlight;
      } finally {
        todayRefreshInFlight = null;
      }
    }

    document.getElementById('dailyGoalSelect')?.addEventListener('change', (e) => {
      const hours = parseFloat(e.target.value);
      if (Number.isNaN(hours)) return;
      setDailyGoalHours(hours);
    });

    document.getElementById('stopTimerBtn')?.addEventListener('click', () => {
      document.getElementById('stopBtn')?.click();
    });

    document.getElementById('playTimerBtn')?.addEventListener('click', () => {
      if (isMonitoring) {
        document.getElementById('pauseBtn')?.click();
      } else if (isPaused) {
        resumeMonitoring();
      } else {
        document.getElementById('startBtn')?.click();
      }
    });

    // ===== RESTORE LINKED PROVIDERS ON STARTUP =====
    async function restoreLinkedProviders() {
      if (!currentEntitlements?.can_integrations) {
        return;
      }
      const epoch = accountEpoch;
      const accountId = currentAccountId;

      if (!guard(epoch, accountId)) return;

      // Jira
      if (localStorage.getItem('flowmates_jira_linked') === 'true') {
        try {
          if (!await loadJiraTasks()) throw new Error('Jira session unavailable');
          if (!guard(epoch, accountId)) return;
          console.log('[Jira] Restored linked session');
        } catch (e) {
          console.log('[Jira] Saved link expired or invalid, clearing:', e);
          localStorage.removeItem('flowmates_jira_linked');
          linkedProviders.jira = false;
        }
      }

      // Linear
      if (localStorage.getItem('flowmates_linear_linked') === 'true') {
        try {
          if (!await loadLinearTasks()) throw new Error('Linear session unavailable');
          if (!guard(epoch, accountId)) return;
          console.log('[Linear] Restored linked session');
        } catch (e) {
          console.log('[Linear] Saved link expired or invalid, clearing:', e);
          localStorage.removeItem('flowmates_linear_linked');
          linkedProviders.linear = false;
        }
      }
    }

    // ===== TEAM FUNCTIONS =====
    function renderTeamStatus(prefix, value) {
      const statusEl = document.getElementById('teamStatus');
      if (!statusEl) return;
      const strong = document.createElement('strong');
      strong.textContent = String(value ?? '');
      statusEl.replaceChildren(document.createTextNode(prefix), strong);
    }

    async function loadUserTeams() {
      const select = document.getElementById('teamSelect');
      const group = document.getElementById('teamSelectorGroup');
      if (!isPaidActive()) {
        if (group) group.classList.add('u-hidden');
        return;
      }
      const epoch = accountEpoch;
      const accountId = currentAccountId;
      try {
        const data = await invoke('get_user_teams');
        if (!guard(epoch, accountId)) return;
        const teams = Array.isArray(data?.teams) ? data.teams : [];
        let activeId = String(data?.active_team_id || '');

        // Clear options
        select.replaceChildren();

        if (teams.length === 0) {
          group.classList.add('u-hidden');
          return;
        }

        // Fallback: if the backend returned no active team (e.g. an older agent
        // version), persist the first membership so session.team_id is never NULL
        // when reports are uploaded.
        if (!activeId && teams[0]?.team_id) {
          activeId = String(teams[0].team_id);
          try {
            await invoke('set_active_team', { teamId: activeId });
            console.log('[Team] Auto-selected first team:', activeId);
          } catch (err) {
            console.error('[Team] Failed auto-select:', err);
          }
        }

        // Populate dropdown
        teams.forEach(t => {
          const teamId = String(t?.team_id || '');
          const opt = document.createElement('option');
          opt.value = teamId;
          opt.textContent = `${teamId.substring(0, 8)}... (${String(t?.role || 'member')})`;
          if (teamId === activeId) opt.selected = true;
          select.appendChild(opt);
        });

        group.classList.remove('u-hidden');

        // Update status
        renderTeamStatus('Active team: ', activeId ? activeId.substring(0, 8) + '...' : 'none');

        console.log('[Team] Loaded', teams.length, 'teams, active:', activeId);
      } catch (e) {
        console.log('[Team] Could not load teams:', e);
        group.classList.add('u-hidden');
      }
    }

    // Team dropdown change handler
    document.getElementById('teamSelect').addEventListener('change', async (e) => {
      const teamId = e.target.value;
      if (!teamId) return;
      try {
        await invoke('set_active_team', { teamId });
        renderTeamStatus('Active team: ', teamId.substring(0, 8) + '...');
        console.log('[Team] Switched active team to:', teamId);
      } catch (err) {
        console.error('[Team] Failed to set active team:', err);
      }
    });

    function setWorkerLoginError(message) {
      const errorEl = document.getElementById('workerLoginError');
      errorEl.textContent = message || '';
      errorEl.classList.toggle('u-hidden', !message);
    }

    async function handleLogin(provider) {
      if (provider === 'jira' || provider === 'linear') {
        if (!requirePaidFeature('integrations', 'Jira and Linear require an Individual or Team license.')) {
          return;
        }
      }

      const btnId = provider === 'google' ? 'loginGoogle' : `login${provider.charAt(0).toUpperCase()}${provider.slice(1)}`;
      const btn = document.getElementById(btnId);
      if (!btn) {
        showToast('Login option unavailable.', 'error');
        return;
      }
      const originalContent = [...btn.childNodes].map((node) => node.cloneNode(true));
      btn.disabled = true;
      setButtonLoading(btn, 'Connecting...');

      try {
        await invoke('start_auth', { provider });
        const pollId = ++accountAuthPollId;
        let authenticated = false;

        for (let attempt = 1; attempt <= 30; attempt += 1) {
          await new Promise((resolve) => setTimeout(resolve, 2000));
          if (pollId !== accountAuthPollId) return;

          setButtonLoading(btn, `Waiting for login... (${attempt}/30)`);
          try {
            const session = await invoke('get_auth_session');
            if (normalizeAccountSession(session, provider)) {
              authenticated = await restoreCloudSession({ refreshEntitlements: true });
            }
          } catch (_) {
            authenticated = false;
          }

          if (authenticated) break;
        }

        if (!authenticated) {
          throw new Error('Login timeout — please try again.');
        }

        hideCloudActivationPanel();
        showMainApp();
        await initMainApp();
        await promptOnboardingIfNeeded();
        showToast(
          isPaidActive()
            ? 'Cloud features activated'
            : 'Signed in — activate your license in Profile',
          'success',
        );
      } catch (e) {
        try { await invoke('cancel_auth'); } catch (_) {}
        const message = e?.message || String(e || 'Login failed');
        showToast(message, 'error', 4500);
      } finally {
        btn.disabled = false;
        btn.replaceChildren(...originalContent.map((node) => node.cloneNode(true)));
      }
    }

    async function handleLogout() {
      // 1. Stop tracking while A's session still exists
      if (isMonitoring || isPaused) {
        try {
          await stopMonitoring();
        } catch (_) {}
      }
      // 2. Cancel any in-flight OAuth
      try { await invoke('cancel_auth'); } catch (_) {}
      // 3. Increment epoch so late async results cannot pollute B's session
      accountEpoch += 1;
      currentAccountId = null;
      // 4. Purge all account-scoped state
      purgeAccountState();
      // 5. Call backend logout
      let logoutError = null;
      try {
        await invoke('logout');
      } catch (e) {
        logoutError = e;
        console.error('Logout failed:', e);
      } finally {
        hideCloudActivationPanel();
        showFreeProfileState();
        showToast(
          logoutError ? 'Local session cleared; remote logout could not be confirmed.' : 'Back to Free (Local) mode',
          logoutError ? 'error' : 'success',
          logoutError ? 4500 : 3000,
        );
      }
    }

    function purgeAccountState() {
      authSession = null;
      currentUser = null;
      currentEntitlements = null;
      linkedProviders = { jira: false, linear: false };
      localStorage.removeItem('flowmates_jira_linked');
      localStorage.removeItem('flowmates_linear_linked');
      localStorage.removeItem('flowmates_team_code');

      const jiraStatus = document.getElementById('jiraLinkStatus');
      const linearStatus = document.getElementById('linearLinkStatus');
      const linkJiraBtn = document.getElementById('linkJiraBtn');
      const linkLinearBtn = document.getElementById('linkLinearBtn');
      if (jiraStatus) jiraStatus.textContent = 'Link Jira';
      if (linearStatus) linearStatus.textContent = 'Link Linear';
      if (linkJiraBtn) linkJiraBtn.classList.remove('button-success');
      if (linkLinearBtn) linkLinearBtn.classList.remove('button-success');

      const teamSelect = document.getElementById('teamSelect');
      if (teamSelect) teamSelect.replaceChildren();
      const teamGroup = document.getElementById('teamSelectorGroup');
      if (teamGroup) teamGroup.classList.add('u-hidden');
      renderTeamStatus('Active team: ', 'none');

      coachMessages = [];
      const coachMsgContainer = document.getElementById('coachMessages');
      if (coachMsgContainer) coachMsgContainer.innerHTML = '';
      coachUsage = null;
      const coachUsageBar = document.getElementById('coachUsageBar');
      if (coachUsageBar) coachUsageBar.style.width = '0%';
      const coachUsageLabel = document.getElementById('coachUsageLabel');
      if (coachUsageLabel) coachUsageLabel.textContent = '';

      todayTotalSeconds = 0;
      updateGoalUI();
      const timerDisplay = document.getElementById('timerDisplay');
      if (timerDisplay) timerDisplay.textContent = '0:00';
      currentHistoryData = null;
      currentWeekData = null;
      const summaryBody = document.getElementById('weeklyReportSummary');
      if (summaryBody) summaryBody.innerHTML = '';

      lastStatusReportPayload = null;
      const reportModal = document.getElementById('statusReportModal');
      if (reportModal) reportModal.classList.remove('visible');
      reportModal.removeAttribute('aria-hidden');
      document.getElementById('statusReportModalBody').innerHTML = '';

      const jiraSelect = document.getElementById('jiraSelect');
      if (jiraSelect) jiraSelect.replaceChildren();
      const linearSelect = document.getElementById('linearSelect');
      if (linearSelect) linearSelect.replaceChildren();
    }

    // Expose login handler to window for inline onclick
    // ===== INTEGRATION LINK HANDLERS =====
    let linkedProviders = { jira: false, linear: false };
    localStorage.removeItem('flowmates_team_code');

    async function linkProvider(provider) {
      if (!requirePaidFeature('integrations', 'Jira and Linear require an Individual or Team license.')) {
        return;
      }

      const btn = document.getElementById('link' + provider.charAt(0).toUpperCase() + provider.slice(1) + 'Btn');
      const statusEl = document.getElementById(provider + 'LinkStatus');
      const originalText = statusEl.textContent;

      btn.disabled = true;
      statusEl.textContent = 'Connecting...';

      try {
        await invoke('start_auth', { provider });
        const pollId = ++providerAuthPollIds[provider];
        let linked = false;

        for (let attempt = 1; attempt <= 30; attempt += 1) {
          await new Promise((resolve) => setTimeout(resolve, 2000));
          if (pollId !== providerAuthPollIds[provider]) return;
          statusEl.textContent = `Linking... (${attempt}/30)`;

          try {
            const command = provider === 'jira' ? 'fetch_jira_tasks' : 'fetch_linear_tasks';
            const tasks = await invoke(command);
            linked = provider === 'jira'
              ? await loadJiraTasks(tasks)
              : await loadLinearTasks(tasks);
          } catch (_) {
            linked = false;
          }

          if (linked) break;
        }

        if (!linked) {
          throw new Error('Link timeout — try again.');
        }

        showToast(`${provider === 'jira' ? 'Jira' : 'Linear'} linked successfully!`, 'success');
      } catch (e) {
        try { await invoke('cancel_auth'); } catch (_) {}
        statusEl.textContent = originalText;
        showToast(e?.message || `Could not link ${provider}.`, 'error');
      } finally {
        btn.disabled = false;
      }
    }

    document.getElementById('linkJiraBtn').onclick = () => linkProvider('jira');
    document.getElementById('linkLinearBtn').onclick = () => linkProvider('linear');

    // Team code handler
    document.getElementById('joinTeamBtn').onclick = async () => {
      if (!requirePaidFeature('sync', 'Joining a team requires a Team license.')) {
        return;
      }

      const code = document.getElementById('teamCodeInput').value.trim();
      const statusEl = document.getElementById('teamStatus');

      if (!code) {
        showToast('Please enter a team code', 'error');
        return;
      }

      const btn = document.getElementById('joinTeamBtn');
      btn.disabled = true;
      btn.textContent = 'Joining...';
      statusEl.textContent = 'Validating code...';

      try {
        const result = await invoke('join_team', { token: code });

        renderTeamStatus('Joined team. ID: ', result?.team_id || 'unknown');
        document.getElementById('teamCodeInput').value = '';
        showToast('Successfully joined the team!', 'success');

        // Refresh teams dropdown
        await loadUserTeams();
      } catch (e) {
        statusEl.textContent = 'Failed to join: ' + e;
        showToast('Error: ' + e, 'error');
      }

      btn.disabled = false;
      btn.textContent = 'Join';
    };

    // ===== TASK LOADING FUNCTIONS =====
    function removeProviderTaskOptions(select, source) {
      select.querySelectorAll('option[data-source]').forEach((option) => {
        if (option.dataset.source === source) option.remove();
      });
      select.querySelectorAll('optgroup').forEach((group) => {
        if (!group.querySelector('option')) group.remove();
      });
    }

    async function loadJiraTasks(prefetchedTasks = null) {
      const select = document.getElementById('jiraSelect');

      try {
        const tasks = Array.isArray(prefetchedTasks)
          ? prefetchedTasks
          : await invoke('fetch_jira_tasks');
        console.log('[UI] Jira tasks:', tasks);

        removeProviderTaskOptions(select, 'jira');

        if (tasks && tasks.length > 0) {
          tasks.forEach((task) => {
            const opt = document.createElement('option');
            opt.value = task.key;
            opt.textContent = `${task.key}: ${task.summary}`;
            opt.dataset.source = 'jira';
            select.appendChild(opt);
          });
        }
        linkedProviders.jira = true;
        document.getElementById('jiraLinkStatus').textContent = 'Linked';
        document.getElementById('linkJiraBtn').classList.add('button-success');
        localStorage.setItem('flowmates_jira_linked', 'true');
        return true;
      } catch (e) {
        console.error('[UI] Failed to load Jira tasks:', e);
        return false;
      }
    }

    async function loadLinearTasks(prefetchedTasks = null) {
      const select = document.getElementById('jiraSelect'); // Using same dropdown for now

      try {
        const tasks = Array.isArray(prefetchedTasks)
          ? prefetchedTasks
          : await invoke('fetch_linear_tasks');
        console.log('[UI] Linear tasks:', tasks);

        removeProviderTaskOptions(select, 'linear');
        if (tasks && tasks.length > 0) {
          tasks.forEach((task) => {
            const opt = document.createElement('option');
            opt.value = task.identifier || task.id;
            opt.textContent = `${task.identifier || task.id}: ${task.title}`;
            opt.dataset.source = 'linear';
            select.appendChild(opt);
          });
        }
        linkedProviders.linear = true;
        document.getElementById('linearLinkStatus').textContent = 'Linked';
        document.getElementById('linkLinearBtn').classList.add('button-success');
        localStorage.setItem('flowmates_linear_linked', 'true');
        return true;
      } catch (e) {
        console.error('[UI] Failed to load Linear tasks:', e);
        return false;
      }
    }

    // Restore linked status from localStorage after entitlements are known

    // Login button handlers (fallback if inline onclick fails)
    document.getElementById('workerLoginForm').onsubmit = handleWorkerLogin;
    document.getElementById('loginGoogle').onclick = () => handleLogin('google');
    document.getElementById('logoutBtn').onclick = handleLogout;
    document.getElementById('activateCloudBtn').onclick = handleActivateCloudClick;
    document.getElementById('closeCloudActivationBtn').onclick = hideCloudActivationPanel;
    document.getElementById('claimLicenseBtn').onclick = handleClaimLicense;

    const COACH_MAX_CHARS = 500;
    let coachMessages = [];
    let coachSending = false;
    let coachUsage = null;

    function canUseCoachChat() {
      return Boolean(
        authSession &&
        currentEntitlements &&
        currentEntitlements.can_cloud_ai &&
        isPaidActive()
      );
    }

    function updateCoachLockState() {
      const shell = document.getElementById('coachChatShell');
      const overlay = document.getElementById('coachLockOverlay');
      const title = document.getElementById('coachLockTitle');
      const text = document.getElementById('coachLockText');
      const btn = document.getElementById('coachUpgradeBtn');
      const locked = !canUseCoachChat();

      if (shell) shell.classList.toggle('is-locked', locked);
      if (overlay) overlay.classList.toggle('visible', locked);

      if (locked && title && text && btn) {
        if (!authSession) {
          title.textContent = 'Upgrade to Pro';
          text.textContent = 'Unlock your AI coach — personalized guidance from your tracked activity.';
          btn.textContent = 'Activate cloud';
        } else if (!isPaidActive()) {
          title.textContent = 'Upgrade to Pro';
          text.textContent = 'Activate your Individual or Team license to chat with your AI coach.';
          btn.textContent = 'Activate license';
        } else {
          title.textContent = 'Pro feature';
          text.textContent = 'Your plan does not include the AI coach yet. Upgrade to Pro to continue.';
          btn.textContent = 'View Profile';
        }
      }
    }

    function renderCoachMarkdown(text) {
      // Escape first; every later replacement introduces only fixed formatting tags.
      let html = escapeHtml(text || '');
      html = html.replace(/^### (.+)$/gm, '<h3>$1</h3>');
      html = html.replace(/^## (.+)$/gm, '<h2>$1</h2>');
      html = html.replace(/^# (.+)$/gm, '<h1>$1</h1>');
      html = html.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
      html = html.replace(/^\d+\.\s+(.+)$/gm, '<li>$1</li>');
      html = html.replace(/^-\s+(.+)$/gm, '<li>$1</li>');
      html = html.replace(/(<li>.*<\/li>\n?)+/g, (block) => `<ul>${block}</ul>`);
      html = html.replace(/\n\n/g, '</p><p>');
      html = `<p>${html}</p>`;
      html = html.replace(/<p>\s*<\/p>/g, '');
      html = html.replace(/<p>(<h[123]>)/g, '$1');
      html = html.replace(/(<\/h[123]>)<\/p>/g, '$1');
      html = html.replace(/<p>(<ul>)/g, '$1');
      html = html.replace(/(<\/ul>)<\/p>/g, '$1');
      return html;
    }

    function renderCoachMessages() {
      const container = document.getElementById('coachMessages');
      if (!container) return;

      if (!coachMessages.length) {
        container.innerHTML = `
          <div class="coach-empty" id="coachEmptyState">
            <div class="coach-empty-title">What's next for your work?</div>
            <div class="coach-empty-sub">Focus, flow, and planning — grounded in your tracked activity.</div>
          </div>`;
        return;
      }

      container.innerHTML = coachMessages.map((msg) => {
        const role = msg.role === 'user' ? 'user' : 'assistant';
        const body = role === 'assistant'
          ? renderCoachMarkdown(msg.content)
          : escapeHtml(msg.content);
        return `
          <div class="coach-msg ${role}">
            <div class="coach-msg-bubble">${body}</div>
          </div>`;
      }).join('');

      container.scrollTop = container.scrollHeight;
    }

    function updateCoachUsageBar() {
      const bar = document.getElementById('coachUsageBar');
      if (!bar) return;
      if (!coachUsage || !coachUsage.limit) {
        bar.classList.add('u-hidden');
        return;
      }
      bar.classList.remove('u-hidden');
      const remainingText = coachUsage.remaining > 0
        ? ` · ${coachUsage.remaining} left`
        : ' · limit reached';
      bar.textContent = `Coach: ${coachUsage.used}/${coachUsage.limit} prompts this month${remainingText}`;
      bar.classList.toggle('warn', coachUsage.limit > 0 && coachUsage.remaining <= Math.ceil(coachUsage.limit * 0.2));
    }

    async function loadCoachChatUsage() {
      if (!canUseCoachChat()) {
        coachUsage = null;
        updateCoachUsageBar();
        return;
      }
      try {
        const data = await invoke('get_coach_chat_usage');
        coachUsage = data?.usage ?? data;
        updateCoachUsageBar();
      } catch (e) {
        console.warn('[Coach] usage load failed:', e);
      }
    }

    async function loadCoachChat() {
      updateCoachLockState();
      if (!canUseCoachChat()) return;
      const epoch = accountEpoch;
      const accountId = currentAccountId;

      try {
        coachMessages = await invoke('get_coach_chat_messages') || [];
        if (!guard(epoch, accountId)) return;
      } catch (e) {
        console.warn('[Coach] messages load failed:', e);
        coachMessages = [];
      }
      renderCoachMessages();
      await loadCoachChatUsage();
    }

    async function sendCoachMessage(text) {
      const trimmed = (text || '').trim();
      if (!trimmed || coachSending) return;
      if (!canUseCoachChat()) {
        updateCoachLockState();
        switchTab('tabCloudInsights');
        return;
      }

      const epoch = accountEpoch;
      const accountId = currentAccountId;

      coachSending = true;
      const input = document.getElementById('coachInput');
      const thinking = document.getElementById('coachThinking');
      const sendBtn = document.getElementById('coachSendBtn');
      if (input) input.value = '';
      updateCoachCharCount();
      if (thinking) thinking.classList.remove('u-hidden');
      if (sendBtn) sendBtn.disabled = true;

      coachMessages.push({
        id: `u-${Date.now()}`,
        role: 'user',
        content: trimmed,
      });
      renderCoachMessages();

      try {
        const result = await invoke('send_coach_chat_message', { message: trimmed });
        if (!guard(epoch, accountId)) return;
        if (Array.isArray(result?.messages)) {
          coachMessages = result.messages;
        } else if (result?.reply) {
          coachMessages.push({
            id: `a-${Date.now()}`,
            role: 'assistant',
            content: result.reply,
          });
        }
        if (result?.usage) {
          coachUsage = result.usage;
          updateCoachUsageBar();
        } else {
          await loadCoachChatUsage();
        }
      } catch (e) {
        coachMessages.push({
          id: `a-${Date.now()}`,
          role: 'assistant',
          content: String(e),
        });
        showToast(String(e), 'error', 4500);
      } finally {
        coachSending = false;
        if (thinking) thinking.classList.add('u-hidden');
        if (sendBtn) sendBtn.disabled = false;
        renderCoachMessages();
      }
    }

    function updateCoachCharCount() {
      const input = document.getElementById('coachInput');
      const counter = document.getElementById('coachCharCount');
      if (!input || !counter) return;
      const len = input.value.length;
      counter.textContent = `${len}/${COACH_MAX_CHARS}`;
      counter.classList.toggle('warn', len >= COACH_MAX_CHARS);
    }

    document.getElementById('coachComposerForm')?.addEventListener('submit', (e) => {
      e.preventDefault();
      const input = document.getElementById('coachInput');
      sendCoachMessage(input?.value || '');
    });

    document.getElementById('coachInput')?.addEventListener('input', updateCoachCharCount);
    document.getElementById('coachInput')?.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        sendCoachMessage(e.target.value);
      }
    });

    document.querySelectorAll('[data-coach-prompt]').forEach((btn) => {
      btn.addEventListener('click', () => {
        if (!canUseCoachChat()) {
          switchTab('tabCloudInsights');
          return;
        }
        sendCoachMessage(btn.getAttribute('data-coach-prompt'));
      });
    });

    document.getElementById('coachUpgradeBtn')?.addEventListener('click', () => {
      if (!authSession) {
        showCloudActivationPanel();
        return;
      }
      if (!isPaidActive()) {
        switchTab('tabProfile');
        document.getElementById('profileLicenseCode')?.focus();
        return;
      }
      switchTab('tabProfile');
    });

    updateCoachCharCount();

    function escapeHtml(text) {
      return String(text ?? '')
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;')
        .replace(/'/g, '&#039;');
    }

    function toFiniteNumber(value, { min = -Infinity, max = Infinity, fallback = 0 } = {}) {
      const parsed = Number(value);
      if (!Number.isFinite(parsed)) return fallback;
      return Math.min(max, Math.max(min, parsed));
    }

    function safeImageUrl(value) {
      if (!value) return '';
      try {
        const url = new URL(String(value));
        return url.protocol === 'https:' ? url.href : '';
      } catch (_) {
        return '';
      }
    }

    function extractEnglishText(text) {
      if (!text) return '';
      const cleaned = String(text).replace(/[\u4E00-\u9FFF\u3400-\u4DBF\u3040-\u30FF\uAC00-\uD7AF]/g, ' ');
      const segments = cleaned
        .split(/(?<=[.!?])\s+|\n+/)
        .map((s) => s.trim())
        .filter(Boolean)
        .filter((seg) => {
          const letters = seg.replace(/[^A-Za-z]/g, '').length;
          const allLetters = seg.replace(/[^A-Za-z\u00C0-\u024F]/g, '').length;
          return allLetters === 0 || letters / allLetters >= 0.55;
        });
      return (segments.length ? segments.join(' ') : cleaned).replace(/\s+/g, ' ').trim();
    }

    function enReport(text) {
      return escapeHtml(extractEnglishText(text));
    }

    // ===== ORIGINAL FUNCTIONS =====
    function getDeviceId() {
      let id = localStorage.getItem('fsk_device_id');
      if (!id) {
        id = 'dev-' + Math.random().toString(36).substr(2, 9);
        localStorage.setItem('fsk_device_id', id);
      }
      return id;
    }

    function log(msg) {
      console.log("[Agent]", msg);
    }

    function showToast(message, type = 'success', duration = 3000) {
      const container = document.getElementById('toastContainer');
      const toast = document.createElement('div');
      const safeType = type === 'error' ? 'error' : 'success';
      toast.className = `toast ${safeType}`;

      const icon = safeType === 'success' ? renderIcon('check', 14) : renderIcon('x', 14);
      const iconEl = document.createElement('span');
      iconEl.className = 'toast-icon';
      iconEl.innerHTML = icon;
      const messageEl = document.createElement('span');
      messageEl.textContent = String(message ?? '');
      toast.append(iconEl, messageEl);

      container.appendChild(toast);

      const safeDuration = toFiniteNumber(duration, { min: 500, max: 30000, fallback: 3000 });
      setTimeout(() => {
        toast.style.animation = 'fadeOut 0.3s ease-out forwards';
        setTimeout(() => toast.remove(), 300);
      }, safeDuration);
    }

    let updateCheckInFlight = false;

    // Checks GitHub Releases for a newer signed build and asks the user before
    // downloading. `manual = true` surfaces "up to date" / error feedback for an
    // explicit "Check for updates" action; the startup check stays silent on no-op.
    async function checkForUpdates({ manual = false } = {}) {
      if (updateCheckInFlight) return;
      updateCheckInFlight = true;
      try {
        const update = await checkAppUpdate();
        if (!update) {
          if (manual) showToast('You are on the latest version.', 'success');
          return;
        }
        showUpdatePrompt(update);
      } catch (err) {
        console.warn('[Updater] Update check failed:', err);
        if (manual) showToast('Could not check for updates. Try again later.', 'error');
      } finally {
        updateCheckInFlight = false;
      }
    }

    function showUpdatePrompt(update) {
      if (document.getElementById('updateOverlay')) return;

      const overlay = document.createElement('div');
      overlay.className = 'modal-overlay';
      overlay.id = 'updateOverlay';

      const notes = String(update.body || '').trim();
      const notesHtml = notes
        ? `<p style="margin-top:10px;font-size:12px;color:hsl(var(--muted-foreground));white-space:pre-wrap;max-height:120px;overflow-y:auto;">${escapeHtml(notes)}</p>`
        : '';

      overlay.innerHTML = `
        <div class="modal-content" style="max-width:340px;">
          <div class="modal-header">
            <span class="modal-title">Update available</span>
          </div>
          <div class="modal-body">
            <p style="font-size:13px;">Flowmates <strong>${escapeHtml(update.version)}</strong> is available.</p>
            ${notesHtml}
            <div id="updateProgress" style="display:none;margin-top:14px;">
              <div style="height:6px;background:hsl(var(--muted));border-radius:999px;overflow:hidden;">
                <div id="updateProgressBar" style="height:100%;width:0%;background:hsl(var(--primary));transition:width 0.2s ease;"></div>
              </div>
              <p id="updateProgressLabel" style="margin-top:6px;font-size:11px;color:hsl(var(--muted-foreground));">Starting download…</p>
            </div>
            <div id="updateActions" style="display:flex;gap:8px;margin-top:16px;justify-content:flex-end;">
              <button id="updateLaterBtn" style="padding:6px 14px;font-size:12px;font-weight:500;border-radius:var(--radius);cursor:pointer;border:1px solid hsl(var(--border));background:hsl(var(--secondary));color:hsl(var(--secondary-foreground));">Later</button>
              <button id="updateNowBtn" style="padding:6px 14px;font-size:12px;font-weight:500;border-radius:var(--radius);cursor:pointer;border:none;background:hsl(var(--primary));color:hsl(var(--primary-foreground));">Update now</button>
            </div>
          </div>
        </div>`;

      document.body.appendChild(overlay);

      const close = () => overlay.remove();
      overlay.querySelector('#updateLaterBtn').addEventListener('click', close);

      overlay.querySelector('#updateNowBtn').addEventListener('click', async () => {
        const actions = overlay.querySelector('#updateActions');
        const progress = overlay.querySelector('#updateProgress');
        const bar = overlay.querySelector('#updateProgressBar');
        const label = overlay.querySelector('#updateProgressLabel');
        actions.classList.add('u-hidden');
        progress.classList.remove('u-hidden');

        let downloaded = 0;
        let total = 0;
        try {
          await update.downloadAndInstall((event) => {
            switch (event.event) {
              case 'Started':
                total = toFiniteNumber(event.data?.contentLength, { min: 0, max: Number.MAX_SAFE_INTEGER });
                label.textContent = 'Downloading…';
                break;
              case 'Progress':
                downloaded += toFiniteNumber(event.data?.chunkLength, { min: 0, max: Number.MAX_SAFE_INTEGER });
                if (total > 0) {
                  const pct = Math.round(toFiniteNumber((downloaded / total) * 100, { min: 0, max: 100, fallback: 0 }));
                  bar.style.width = pct + '%';
                  label.textContent = `Downloading… ${pct}%`;
                }
                break;
              case 'Finished':
                bar.style.width = '100%';
                label.textContent = 'Installing… the app will restart.';
                break;
            }
          });
          await relaunch();
        } catch (err) {
          console.error('[Updater] Install failed:', err);
          progress.classList.add('u-hidden');
          actions.classList.remove('u-hidden');
          showToast('Update failed to install. Please try again.', 'error', 6000);
        }
      });
    }

    async function loadConfig() {
      const config = await invoke('get_config');
      const configuredInterval = Number(config.captureInterval);
      captureIntervalMs = Number.isFinite(configuredInterval) && configuredInterval >= 5000
        ? configuredInterval
        : 60000;
      document.getElementById('statInterval').textContent = captureIntervalMs / 1000 + 's';

      let hours = config.dailyGoalHours;
      if (hours == null) {
        const legacy = localStorage.getItem('flowmates_daily_goal_hours');
        if (legacy) {
          const parsed = parseFloat(legacy);
          if (!Number.isNaN(parsed)) {
            hours = parsed;
            await invoke('update_config', { patch: { dailyGoalHours: parsed } });
            localStorage.removeItem('flowmates_daily_goal_hours');
          }
        }
      }
      dailyGoalHours = toFiniteNumber(hours, { min: 0, max: 24, fallback: 6 });
      syncDailyGoalSelect();
      updateGoalUI();
    }

    async function saveConfig() {
      await invoke('update_config', {
        patch: {
          captureInterval: captureIntervalMs,
          visionModel: 'Flowmates/local-vision'
        }
      });
      console.log("[Config] vision model:", "Flowmates/local-vision");
    }

    /** Absolute macOS paths resolved by the backend. */
    async function getFlowmatesResolvedPaths() {
      try {
        const p = await invoke("get_flowmates_user_paths");
        return p && typeof p === "object" ? p : null;
      } catch {
        return null;
      }
    }

    async function startMonitoringFull({ resume = false } = {}) {
      const actionId = ++monitoringActionId;
      const btn = document.getElementById('playTimerBtn');
      btn.disabled = true;
      btn.classList.add('loading-state');

      const overlay = document.getElementById('setupOverlay');
      const progress = document.getElementById('setupProgress');
      const statusText = document.getElementById('setupStatus');
      const subtext = document.getElementById('setupSubtext');

      const showStep = (status, sub = "Initializing...", pct = 0) => {
        overlay.style.display = 'flex';
        overlay.classList.remove('u-hidden');
        statusText.textContent = status;
        subtext.textContent = sub;
        progress.style.width = toFiniteNumber(pct, { min: 0, max: 100, fallback: 0 }) + '%';
      };

      let unlistenServerProgress = null;
      let unlistenServerReady = null;
      let unlistenServerError = null;
      let serverTimeoutId = null;
      try {
        showStep("Starting local vision…", "Preparing local server…", 20);

        unlistenServerProgress = await listen('server-start-progress', (event) => {
          const { status, pct, step } = event.payload || {};
          if (status) showStep(status, statusText.textContent, pct ?? 20);
        });

        let resolveServerEvent;
        const serverEventPromise = new Promise((resolve) => {
          resolveServerEvent = resolve;
        });
        unlistenServerReady = await listen('server-start-ready', (event) => {
          resolveServerEvent({ payload: event.payload });
        });
        unlistenServerError = await listen('server-start-error', (event) => {
          resolveServerEvent({ error: event.payload?.error || 'Server start failed' });
        });

        const res = await invoke('start_server');
        if (monitoringActionId !== actionId) return;
        console.log("Server Start Result:", res);

        if (res.status === 'already_running') {
          // Server was already up — skip event wait.
        } else if (res.status === 'started_async') {
          const timeoutPromise = new Promise((_, reject) => {
            serverTimeoutId = setTimeout(
              () => reject(new Error('Server start timed out after 280s')),
              280_000,
            );
          });
          const serverEvent = await Promise.race([
            serverEventPromise,
            timeoutPromise,
          ]);
          if (serverEvent.error) throw new Error(serverEvent.error);
          const asyncResult = serverEvent.payload || {};
          console.log("Server Ready:", asyncResult);
          Object.assign(res, asyncResult);
        } else {
          throw new Error("Unexpected server status: " + JSON.stringify(res));
        }

        overlay.classList.add('u-hidden');
        const layerSuffix =
          typeof res.gpuLayers === 'number'
            ? ` (${res.gpuLayers} GPU layers${res.gpuAuto ? ', auto-selected' : ''})`
            : '';
        showToast(`Local AI Server Ready${layerSuffix}`, 'success');

        try {
          const history = await invoke('get_today_history');
          if (monitoringActionId !== actionId) return;
          todayTotalSeconds = Math.max(todayTotalSeconds, history.total_seconds || 0);
        } catch (_) { /* keep local total */ }

        isMonitoring = true;
        isPaused = false;
        captureRunId += 1;
        const runId = captureRunId;
        document.getElementById('statStatus').textContent = 'On';
        document.getElementById('statStatus').style.color = 'hsl(142.1 76.2% 36.3%)';
        const badge = document.getElementById('monitoringBadge');
        if (badge) { badge.textContent = 'On'; badge.className = 'badge badge-success'; }

        if (resume) resumeSessionTimer();
        else startSessionTimer();
        updatePlayButtonState();

        scheduleMonitoringCaptures(runId);
        ollamaConfirmedOnline = true;
        await checkOllama();
        if (monitoringActionId !== actionId) return;
      } catch (e) {
        if (resume) isPaused = true;
        isMonitoring = false;
        overlay.classList.add('u-hidden');
        console.error("Start Failed:", e);
        showToast("Error: " + e, "error");
      } finally {
        if (serverTimeoutId) clearTimeout(serverTimeoutId);
        safeUnlisten(unlistenServerProgress);
        safeUnlisten(unlistenServerReady);
        safeUnlisten(unlistenServerError);
        btn.disabled = false;
        btn.classList.remove('loading-state');
        updatePlayButtonState();
      }
    }

    document.getElementById('startBtn').onclick = async () => {
      if (isPaused) {
        await resumeMonitoring();
        return;
      }
      await startMonitoringFull();
    };

    document.getElementById('pauseBtn').onclick = async () => {
      if (isMonitoring) await pauseMonitoring();
    };

    document.getElementById('stopBtn').onclick = async () => {
      if (isMonitoring || isPaused) await stopMonitoring();
    };

    let ollamaConfirmedOnline = false;
    let ollamaLastCheckedAt = 0;
    let ollamaCheckInFlight = null;
    const OLLAMA_ONLINE_RECHECK_MS = 120000;

    async function checkOllama() {
      if (ollamaCheckInFlight) return ollamaCheckInFlight;

      ollamaCheckInFlight = (async () => {
        ollamaLastCheckedAt = Date.now();
        const dot = document.getElementById('ollamaDot');
        const txt = document.getElementById('ollamaStatus');
        try {
          const status = await invoke('check_local_server');
          if (status.online) {
            dot.className = 'status-dot active';
            txt.textContent = 'Local Server Ready';
            ollamaConfirmedOnline = true;
          } else {
            dot.className = 'status-dot';
            txt.textContent = 'Server Offline';
            ollamaConfirmedOnline = false;
          }
          return status;
        } catch (e) {
          console.error(e);
          dot.className = 'status-dot';
          txt.textContent = 'Server Offline';
          ollamaConfirmedOnline = false;
          return { online: false };
        }
      })();

      try {
        return await ollamaCheckInFlight;
      } finally {
        ollamaCheckInFlight = null;
      }
    }

    setInterval(() => {
      if (!ollamaConfirmedOnline || Date.now() - ollamaLastCheckedAt >= OLLAMA_ONLINE_RECHECK_MS) {
        checkOllama();
      }
    }, 30000);

    async function initMainApp() {
      await loadConfig();
      await checkOllama();
      updateGoalUI();
      refreshTodayView();
      if (todayRefreshInterval) clearInterval(todayRefreshInterval);
      todayRefreshInterval = setInterval(refreshTodayView, 60000);
    }

    let userPreferences = null;

    const ONBOARDING_ROLE_OPTIONS = [
      { id: 'software_developer', label: 'Software development', icon: '💻' },
      { id: 'design', label: 'Design & UX', icon: '🎨' },
      { id: 'product', label: 'Product / project management', icon: '📋' },
      { id: 'data', label: 'Data & analytics', icon: '📊' },
      { id: 'student', label: 'Learning / student work', icon: '📚' },
      { id: 'other', label: 'Other knowledge work', icon: '✨' },
    ];

    const ONBOARDING_ACTIVITY_OPTIONS = [
      { id: 'coding', label: 'Writing & shipping code', icon: '⌨️' },
      { id: 'debugging', label: 'Debugging & fixing issues', icon: '🐛' },
      { id: 'code_review', label: 'Code review & collaboration', icon: '👀' },
      { id: 'meetings', label: 'Meetings & calls', icon: '📞' },
      { id: 'planning', label: 'Planning & documentation', icon: '📝' },
      { id: 'research', label: 'Research & learning', icon: '🔍' },
      { id: 'design_work', label: 'Design & prototyping', icon: '🖌️' },
      { id: 'support', label: 'Support & operations', icon: '🛠️' },
    ];

    const ONBOARDING_GOAL_OPTIONS = [
      { id: 'distractions', label: 'Spend less time on distracting apps', icon: '🌙' },
      { id: 'time_awareness', label: 'Understand how I spend my time and make better decisions', icon: '📊' },
      { id: 'focus', label: 'Improve my attention and deep focus', icon: '🧩' },
      { id: 'balance', label: 'Improve work-life balance', icon: '⚖️' },
      { id: 'accountability', label: 'Track progress on goals and tickets', icon: '🎯' },
      { id: 'wellbeing', label: 'Feel good about my productivity', icon: '💡' },
    ];

    const ONBOARDING_GOAL_HOURS = [4, 5, 6, 7, 8];

    const onboardingState = {
      step: 0,
      displayName: '',
      workRoles: [],
      customJob: '',
      workActivities: [],
      improvementGoals: [],
      dailyGoalHours: 6,
      reopening: false,
    };

    function onboardingRoleLabel(id, customJob) {
      if (id === 'other' && customJob?.trim()) return customJob.trim();
      return ONBOARDING_ROLE_OPTIONS.find((o) => o.id === id)?.label || id;
    }

    function onboardingFirstName() {
      const n = onboardingState.displayName.trim() || 'there';
      return n.split(' ')[0];
    }

    function renderWorkPreferencesSummary(prefs) {
      const el = document.getElementById('workPreferencesSummary');
      if (!el) return;
      if (!prefs?.onboardingCompleted) {
        el.textContent = authSession
          ? 'Complete the setup wizard to personalize reports and greetings.'
          : 'Sign in to cloud features to set up your work preferences.';
        return;
      }
      const roleLabels = (prefs.workRoles || [])
        .map((id) => onboardingRoleLabel(id, prefs.customJob))
        .join(', ');
      const goalLabels = (prefs.improvementGoals || [])
        .map((id) => ONBOARDING_GOAL_OPTIONS.find((o) => o.id === id)?.label || id)
        .slice(0, 2)
        .join('; ');
      el.textContent = [
        prefs.displayName ? `Name: ${prefs.displayName}` : null,
        roleLabels ? `Focus: ${roleLabels}` : null,
        goalLabels ? `Goals: ${goalLabels}` : null,
        prefs.dailyGoalHours != null ? `Daily goal: ${prefs.dailyGoalHours}h` : null,
      ].filter(Boolean).join(' · ');
    }

    async function loadUserPreferences() {
      try {
        userPreferences = await invoke('get_user_preferences');
      } catch (_) {
        userPreferences = { onboardingCompleted: false };
      }
      renderWorkPreferencesSummary(userPreferences);
      return userPreferences;
    }

    async function persistUserPreferences(partial, completed = false) {
      const prefs = {
        onboardingCompleted: completed,
        displayName: partial.displayName ?? onboardingState.displayName ?? userPreferences?.displayName ?? null,
        workRoles: partial.workRoles ?? onboardingState.workRoles ?? userPreferences?.workRoles ?? [],
        customJob: (() => {
          const roles = partial.workRoles ?? onboardingState.workRoles ?? userPreferences?.workRoles ?? [];
          if (!roles.includes('other')) return null;
          const job = (partial.customJob ?? onboardingState.customJob ?? userPreferences?.customJob ?? '').trim();
          return job || null;
        })(),
        workActivities: partial.workActivities ?? onboardingState.workActivities ?? userPreferences?.workActivities ?? [],
        improvementGoals: partial.improvementGoals ?? onboardingState.improvementGoals ?? userPreferences?.improvementGoals ?? [],
        dailyGoalHours: toFiniteNumber(
          partial.dailyGoalHours ?? onboardingState.dailyGoalHours ?? userPreferences?.dailyGoalHours,
          { min: 0, max: 24, fallback: 6 },
        ),
      };
      userPreferences = await invoke('save_user_preferences_command', { prefs });
      if (prefs.dailyGoalHours != null) {
        await setDailyGoalHours(prefs.dailyGoalHours);
      }
      renderWorkPreferencesSummary(userPreferences);
      updateTodayGreeting();
      return userPreferences;
    }

    function onboardingCanContinue() {
      switch (onboardingState.step) {
        case 0: return onboardingState.displayName.trim().length >= 2;
        case 1: {
          if (onboardingState.workRoles.length === 0) return false;
          if (onboardingState.workRoles.includes('other')) {
            return onboardingState.customJob.trim().length >= 2;
          }
          return true;
        }
        case 2: return onboardingState.workActivities.length > 0;
        case 3: return onboardingState.improvementGoals.length > 0;
        case 4: return onboardingState.dailyGoalHours != null;
        default: return false;
      }
    }

    function onboardingToggleWorkRole(id) {
      const arr = onboardingState.workRoles;
      const idx = arr.indexOf(id);
      if (idx >= 0) {
        arr.splice(idx, 1);
        if (id === 'other') onboardingState.customJob = '';
      } else {
        arr.push(id);
      }
      renderOnboardingStep();
    }

    function onboardingToggleMulti(key, id) {
      const arr = onboardingState[key];
      const idx = arr.indexOf(id);
      if (idx >= 0) arr.splice(idx, 1);
      else arr.push(id);
      renderOnboardingStep();
    }

    function renderOnboardingStep() {
      const body = document.getElementById('onboardingBody');
      const continueBtn = document.getElementById('onboardingContinueBtn');
      const backBtn = document.getElementById('onboardingBackBtn');
      const fill = document.getElementById('onboardingProgressFill');
      if (!body || !continueBtn) return;

      const step = onboardingState.step;
      fill.style.width = `${((step + 1) / 5) * 100}%`;
      backBtn.style.visibility = step === 0 ? 'hidden' : 'visible';

      if (step === 0) {
        body.innerHTML = `
          <h1 class="onboarding-title">Welcome to Flowmates</h1>
          <p class="onboarding-subtitle">Let's personalize your experience. What should we call you?</p>
          <input type="text" class="onboarding-name-input" id="onboardingNameInput"
            placeholder="Your first name" maxlength="40" value="${escapeHtml(onboardingState.displayName)}" autocomplete="name">`;
        const input = document.getElementById('onboardingNameInput');
        input?.focus();
        input?.addEventListener('input', (e) => {
          onboardingState.displayName = e.target.value;
          continueBtn.disabled = !onboardingCanContinue();
        });
        continueBtn.textContent = 'Continue';
      } else if (step === 1) {
        const showOtherJob = onboardingState.workRoles.includes('other');
        body.innerHTML = `
          <h1 class="onboarding-title">What kind of work do you do, ${escapeHtml(onboardingFirstName())}?</h1>
          <p class="onboarding-subtitle">Pick one or more that describe your role.</p>
          <div class="onboarding-options">${ONBOARDING_ROLE_OPTIONS.map((o) => `
            <button type="button" class="onboarding-option ${onboardingState.workRoles.includes(o.id) ? 'selected' : ''}"
              data-onboard-role="${escapeHtml(o.id)}">
              <span class="onboarding-check">${onboardingState.workRoles.includes(o.id) ? '✓' : ''}</span>
              <span class="onboarding-option-text">${escapeHtml(o.label)}</span>
              <span class="onboarding-option-icon">${escapeHtml(o.icon)}</span>
            </button>`).join('')}</div>
          ${showOtherJob ? `
          <div class="onboarding-other-job">
            <label for="onboardingCustomJobInput">Describe your job</label>
            <input type="text" class="onboarding-name-input" id="onboardingCustomJobInput"
              placeholder="e.g. Technical writer, QA engineer, Founder…" maxlength="80"
              value="${escapeHtml(onboardingState.customJob)}" autocomplete="organization-title">
          </div>` : ''}`;
        body.querySelectorAll('[data-onboard-role]').forEach((btn) => {
          btn.onclick = () => onboardingToggleWorkRole(btn.dataset.onboardRole);
        });
        const jobInput = document.getElementById('onboardingCustomJobInput');
        if (jobInput) {
          jobInput.focus();
          jobInput.addEventListener('input', (e) => {
            onboardingState.customJob = e.target.value;
            continueBtn.disabled = !onboardingCanContinue();
          });
        }
        if (showOtherJob && !onboardingState.customJob.trim()) {
          continueBtn.textContent = 'Enter your job to continue';
        } else {
          continueBtn.textContent = onboardingState.workRoles.length ? 'Continue' : 'Pick at least one';
        }
      } else if (step === 2) {
        body.innerHTML = `
          <h1 class="onboarding-title">What fills your workday?</h1>
          <p class="onboarding-subtitle">Select the activities you spend the most time on.</p>
          <div class="onboarding-options">${ONBOARDING_ACTIVITY_OPTIONS.map((o) => `
            <button type="button" class="onboarding-option ${onboardingState.workActivities.includes(o.id) ? 'selected' : ''}"
              data-onboard-activity="${escapeHtml(o.id)}">
              <span class="onboarding-check">${onboardingState.workActivities.includes(o.id) ? '✓' : ''}</span>
              <span class="onboarding-option-text">${escapeHtml(o.label)}</span>
              <span class="onboarding-option-icon">${escapeHtml(o.icon)}</span>
            </button>`).join('')}</div>`;
        body.querySelectorAll('[data-onboard-activity]').forEach((btn) => {
          btn.onclick = () => onboardingToggleMulti('workActivities', btn.dataset.onboardActivity);
        });
        continueBtn.textContent = onboardingState.workActivities.length ? 'Continue' : 'Pick at least one';
      } else if (step === 3) {
        body.innerHTML = `
          <h1 class="onboarding-title">What do you want to improve?</h1>
          <p class="onboarding-subtitle">Pick one or more goals — we'll tailor insights and reports to these.</p>
          <div class="onboarding-options">${ONBOARDING_GOAL_OPTIONS.map((o) => `
            <button type="button" class="onboarding-option ${onboardingState.improvementGoals.includes(o.id) ? 'selected' : ''}"
              data-onboard-goal="${escapeHtml(o.id)}">
              <span class="onboarding-check">${onboardingState.improvementGoals.includes(o.id) ? '✓' : ''}</span>
              <span class="onboarding-option-text">${escapeHtml(o.label)}</span>
              <span class="onboarding-option-icon">${escapeHtml(o.icon)}</span>
            </button>`).join('')}</div>`;
        body.querySelectorAll('[data-onboard-goal]').forEach((btn) => {
          btn.onclick = () => onboardingToggleMulti('improvementGoals', btn.dataset.onboardGoal);
        });
        continueBtn.textContent = onboardingState.improvementGoals.length ? 'Continue' : 'Pick at least one';
      } else if (step === 4) {
        body.innerHTML = `
          <h1 class="onboarding-title">Set your daily tracking goal</h1>
          <p class="onboarding-subtitle">How many focused hours do you want to track on a typical day?</p>
          <div class="onboarding-options">${ONBOARDING_GOAL_HOURS.map((h) => `
            <button type="button" class="onboarding-option ${onboardingState.dailyGoalHours === h ? 'selected' : ''}"
              data-onboard-hours="${toFiniteNumber(h, { min: 0, max: 24, fallback: 6 })}">
              <span class="onboarding-check">${onboardingState.dailyGoalHours === h ? '✓' : ''}</span>
              <span class="onboarding-option-text">${toFiniteNumber(h, { min: 0, max: 24, fallback: 6 })} hours per day</span>
              <span class="onboarding-option-icon">⏱️</span>
            </button>`).join('')}</div>`;
        body.querySelectorAll('[data-onboard-hours]').forEach((btn) => {
          btn.onclick = () => {
            onboardingState.dailyGoalHours = Number(btn.dataset.onboardHours);
            renderOnboardingStep();
          };
        });
        continueBtn.textContent = 'Finish setup';
      }

      continueBtn.disabled = !onboardingCanContinue();
    }

    function showOnboardingWizard(fromProfile = false) {
      onboardingState.reopening = fromProfile;
      onboardingState.step = 0;
      if (userPreferences) {
        onboardingState.displayName = userPreferences.displayName || '';
        onboardingState.workRoles = [...(userPreferences.workRoles || [])];
        onboardingState.customJob = userPreferences.customJob || '';
        onboardingState.workActivities = [...(userPreferences.workActivities || [])];
        onboardingState.improvementGoals = [...(userPreferences.improvementGoals || [])];
        onboardingState.dailyGoalHours = toFiniteNumber(
          userPreferences.dailyGoalHours,
          { min: 0, max: 24, fallback: 6 },
        );
      }
      if (!onboardingState.displayName.trim()) {
        onboardingState.displayName =
          currentUser?.display_name || currentUser?.name || currentUser?.email?.split('@')[0] || '';
      }
      const overlay = document.getElementById('onboardingOverlay');
      overlay?.classList.add('visible');
      overlay?.setAttribute('aria-hidden', 'false');
      renderOnboardingStep();
    }

    function hideOnboardingWizard() {
      const overlay = document.getElementById('onboardingOverlay');
      overlay?.classList.remove('visible');
      overlay?.setAttribute('aria-hidden', 'true');
    }

    async function onboardingContinue() {
      if (!onboardingCanContinue()) return;
      if (onboardingState.step < 4) {
        await persistUserPreferences({}, false);
        onboardingState.step += 1;
        renderOnboardingStep();
        return;
      }
      const continueBtn = document.getElementById('onboardingContinueBtn');
      if (continueBtn) {
        continueBtn.disabled = true;
        continueBtn.textContent = 'Saving…';
      }
      try {
        await persistUserPreferences({}, true);
        hideOnboardingWizard();
        if (!onboardingState.reopening) {
          await restoreCloudSession();
          showMainApp();
          await initMainApp();
        } else {
          showToast('Work preferences updated', 'success');
        }
      } catch (e) {
        showToast('Could not save preferences: ' + e, 'error');
      } finally {
        const btn = document.getElementById('onboardingContinueBtn');
        if (btn) {
          btn.disabled = !onboardingCanContinue();
          btn.textContent = onboardingState.step === 4 ? 'Finish setup' : 'Continue';
        }
      }
    }

    async function promptOnboardingIfNeeded() {
      if (!authSession && !currentUser) return false;
      await loadUserPreferences();
      if (userPreferences?.onboardingCompleted) return false;
      showOnboardingWizard(false);
      return true;
    }

    document.getElementById('onboardingContinueBtn')?.addEventListener('click', () => {
      onboardingContinue().catch((e) => showToast(String(e), 'error'));
    });
    document.getElementById('onboardingBackBtn')?.addEventListener('click', () => {
      if (onboardingState.step > 0) {
        onboardingState.step -= 1;
        renderOnboardingStep();
      }
    });
    document.getElementById('editWorkPreferencesBtn')?.addEventListener('click', () => {
      showOnboardingWizard(true);
    });

    let analyticsConsent = null;
    let analyticsConsentBusy = false;

    function renderAnalyticsConsentSettings() {
      const toggle = document.getElementById('analyticsConsentToggle');
      const status = document.getElementById('analyticsConsentStatus');
      if (!toggle || !status) return;

      if (!analyticsConsent?.decided) {
        toggle.checked = false;
        toggle.indeterminate = false;
        status.textContent = 'You have not chosen yet. The prompt appears on first launch.';
        return;
      }

      toggle.checked = Boolean(analyticsConsent.consented);
      toggle.indeterminate = false;
      status.textContent = analyticsConsent.consented
        ? 'Anonymous usage data is shared to help improve Flowmates. You can turn this off anytime.'
        : 'Anonymous usage data is not shared. Flowmates remains fully local.';
    }

    async function loadAnalyticsConsent() {
      try {
        analyticsConsent = await invoke('get_analytics_consent');
      } catch (_) {
        analyticsConsent = { decided: false, consented: false };
      }
      renderAnalyticsConsentSettings();
      return analyticsConsent;
    }

    function showAnalyticsConsentModal() {
      const overlay = document.getElementById('analyticsConsentOverlay');
      overlay?.classList.add('visible');
      overlay?.setAttribute('aria-hidden', 'false');
    }

    function hideAnalyticsConsentModal() {
      const overlay = document.getElementById('analyticsConsentOverlay');
      overlay?.classList.remove('visible');
      overlay?.setAttribute('aria-hidden', 'true');
    }

    async function applyAnalyticsConsent(consented, { fromFirstLaunch = false } = {}) {
      if (analyticsConsentBusy) return;
      analyticsConsentBusy = true;
      const acceptBtn = document.getElementById('analyticsConsentAcceptBtn');
      const declineBtn = document.getElementById('analyticsConsentDeclineBtn');
      const toggle = document.getElementById('analyticsConsentToggle');
      acceptBtn?.setAttribute('disabled', 'true');
      declineBtn?.setAttribute('disabled', 'true');
      if (toggle) toggle.disabled = true;

      try {
        analyticsConsent = await invoke('set_analytics_consent', { consented });
        renderAnalyticsConsentSettings();
        if (fromFirstLaunch) {
          hideAnalyticsConsentModal();
        }
        if (consented) {
          invoke('sync_anonymous_analytics').catch(() => {});
          if (fromFirstLaunch) {
            showToast('Thanks for helping improve Flowmates', 'success');
          }
        } else if (!fromFirstLaunch) {
          showToast('Anonymous analytics turned off', 'success');
        }
        if (fromFirstLaunch) {
          await promptOnboardingIfNeeded();
        }
      } catch (e) {
        showToast('Could not save analytics preference: ' + e, 'error');
      } finally {
        analyticsConsentBusy = false;
        acceptBtn?.removeAttribute('disabled');
        declineBtn?.removeAttribute('disabled');
        if (toggle) toggle.disabled = false;
      }
    }

    async function promptAnalyticsConsentIfNeeded() {
      await loadAnalyticsConsent();
      if (analyticsConsent?.decided) return false;
      showAnalyticsConsentModal();
      return true;
    }

    document.getElementById('analyticsConsentAcceptBtn')?.addEventListener('click', () => {
      applyAnalyticsConsent(true, { fromFirstLaunch: true }).catch((e) => showToast(String(e), 'error'));
    });
    document.getElementById('analyticsConsentDeclineBtn')?.addEventListener('click', () => {
      applyAnalyticsConsent(false, { fromFirstLaunch: true }).catch((e) => showToast(String(e), 'error'));
    });
    document.getElementById('analyticsConsentToggle')?.addEventListener('change', (event) => {
      const next = event.target.checked;
      applyAnalyticsConsent(next).catch((e) => {
        showToast(String(e), 'error');
        renderAnalyticsConsentSettings();
      });
    });
    async function submitAppFeedback() {
      const input = document.getElementById('appFeedbackInput');
      const btn = document.getElementById('sendFeedbackBtn');
      const message = input?.value?.trim() || '';
      if (message.length < 3) {
        showToast('Please enter at least a few words of feedback.', 'error');
        return;
      }
      if (btn) btn.disabled = true;
      try {
        await invoke('submit_product_feedback', { message });
        if (input) input.value = '';
        showToast('Thanks — your feedback was saved.', 'success');
      } catch (e) {
        showToast('Could not send feedback: ' + e, 'error');
      } finally {
        if (btn) btn.disabled = false;
      }
    }

    document.getElementById('sendFeedbackBtn')?.addEventListener('click', () => {
      submitAppFeedback().catch((e) => showToast(String(e), 'error'));
    });

    async function init() {
      const playBtn = document.getElementById('playTimerBtn');
      try {
        console.log("[Agent] Initializing backend...");
        await invoke('initialize_agent');
        console.log("[Agent] Backend initialized.");
      } catch (e) {
        console.error("Failed to init agent:", e);
        showToast(
          typeof e === "string"
            ? e
            : "Flowmates cannot write its Application Support data folder. Check its ownership and permissions, and verify that the disk has free space. Details: " + e,
          "error",
          10000
        );
      } finally {
        if (playBtn) playBtn.disabled = false;
      }

      await loadUserPreferences();
      showMainApp();
      await restoreCloudSession();
      showMainApp();
      await initMainApp();
      await loadAnalyticsConsent();
      const waitingForConsent = await promptAnalyticsConsentIfNeeded();
      if (!waitingForConsent) {
        if (analyticsConsent?.consented) {
          invoke('sync_anonymous_analytics').catch(() => {});
        }
        await promptOnboardingIfNeeded();
      }

      // Background self-update check; never blocks startup and stays silent on no-op.
      checkForUpdates().catch(() => {});
    }

    init();

    async function refreshTasks() {
      if (!isPaidActive() || !currentEntitlements?.can_integrations) {
        return;
      }

      const sel = document.getElementById('jiraSelect');
      sel.replaceChildren(new Option('Loading...', ''));

      try {
        let tasks = [];
        const hasJira = linkedProviders.jira;
        const hasLinear = linkedProviders.linear;

        if (hasJira) {
          const jiraTasks = await invoke('fetch_jira_tasks');
          tasks = tasks.concat(jiraTasks.map(t => ({
            key: t.key,
            title: t.summary,
            status: t.status || 'Unknown',
            source: 'jira'
          })));
        }

        if (hasLinear) {
          const linearTasks = await invoke('fetch_linear_tasks');
          tasks = tasks.concat(linearTasks.map(t => ({
            key: t.identifier,
            title: t.title,
            status: t.state || 'Unknown',
            source: 'linear'
          })));
        }

        sel.replaceChildren(
          new Option('Select Task...', ''),
          new Option('General / No Ticket', 'General'),
        );

        if (tasks.length > 0) {
          const groups = {};
          tasks.forEach(t => {
            const s = t.status;
            if (!groups[s]) groups[s] = [];
            groups[s].push(t);
          });

          const priority = ["In Progress", "Started", "To Do", "Todo", "Backlog", "Done", "Completed"];
          const sortedKeys = Object.keys(groups).sort((a, b) => {
            const idxA = priority.findIndex(p => a.toLowerCase().includes(p.toLowerCase()));
            const idxB = priority.findIndex(p => b.toLowerCase().includes(p.toLowerCase()));
            if (idxA !== -1 && idxB !== -1) return idxA - idxB;
            if (idxA !== -1) return -1;
            if (idxB !== -1) return 1;
            return a.localeCompare(b);
          });

          sortedKeys.forEach(status => {
            const grp = document.createElement('optgroup');
            grp.label = status;

            groups[status].forEach(t => {
              const opt = document.createElement('option');
              opt.value = t.key;
              opt.textContent = `[${t.key}] ${t.title}`;
              opt.dataset.source = t.source;
              grp.appendChild(opt);
            });
            sel.appendChild(grp);
          });
        }

        const other = document.createElement('option');
        other.value = "MANUAL";
        other.textContent = "Other (Type Manual)";
        sel.appendChild(other);

      } catch (e) {
        console.error('[UI] Refresh tasks failed:', e);
        sel.replaceChildren(
          new Option('Not connected', ''),
          new Option('General / No Ticket', 'General'),
        );
        const manual = document.createElement('option');
        manual.value = "MANUAL";
        manual.textContent = "Manual Task";
        sel.appendChild(manual);
      }
    }

    const refreshJira = refreshTasks;

    document.getElementById('jiraSelect').onchange = (e) => {
      if (e.target.value === 'MANUAL') {
        document.getElementById('manualTask').classList.remove('u-hidden');
      } else {
        document.getElementById('manualTask').classList.add('u-hidden');
      }
      const source = e.target.selectedOptions?.[0]?.dataset.source;
      const syncWrapper = document.getElementById('syncToJiraWrapper');
      if (syncWrapper) {
        syncWrapper.classList.toggle('u-hidden', !(currentEntitlements?.can_integrations && source === 'jira'));
      }
    };

    let isSyncing = false;
    let activitySaveInFlight = null;
    let lastGoodSnapshot = null;
    let captureRetryTimeout = null;
    const CAPTURE_RETRY_DELAY_MS = 2500;
    const MIN_PERSISTABLE_ACTIVITY_SECONDS = 30;

    function clearCaptureRetry() {
      if (captureRetryTimeout) {
        clearTimeout(captureRetryTimeout);
        captureRetryTimeout = null;
      }
    }

    function isAnalysisFailure(snapshot) {
      if (!snapshot) return true;
      if (snapshot.analysis_failed === true) return true;
      const desc = (snapshot.description || '').trim();
      return desc.startsWith('Screen analysis failed')
        || desc === 'No analysis available';
    }

    function getTaskContext() {
      let task = 'General';
      let jiraTicket = null;
      const sel = document.getElementById('jiraSelect');
      const manualTask = document.getElementById('manualTask');

      if (!currentEntitlements?.can_integrations) {
        task = manualTask?.value?.trim() || 'General';
        return { task, jiraTicket };
      }

      if (sel.value && sel.value !== 'MANUAL') {
        const selected = sel.options[sel.selectedIndex];
        task = selected?.text || 'General';
        const attachToJira = document.getElementById('syncToJira')?.checked !== false;
        if (attachToJira && selected?.dataset.source === 'jira') {
          jiraTicket = sel.value;
        }
      } else if (sel.value === 'MANUAL') {
        task = manualTask?.value || 'Manual Work';
      }
      return { task, jiraTicket };
    }

    async function recordActivity(description, category, jiraTicket, { toast = false } = {}) {
      if (activitySaveInFlight) {
        await activitySaveInFlight.catch(() => {});
      }

      const elapsed = pendingActivitySeconds();
      if (elapsed < MIN_PERSISTABLE_ACTIVITY_SECONDS) {
        return false;
      }

      activitySaveInFlight = invoke('save_activity', {
        description,
        activityType: category,
        jiraTicket: jiraTicket,
        durationSeconds: elapsed
      });
      try {
        await activitySaveInFlight;
      } finally {
        activitySaveInFlight = null;
      }
      consumePendingActivity(elapsed);

      const statSent = document.getElementById('statSent');
      if (statSent) {
        statSent.textContent = parseInt(statSent.textContent || '0', 10) + 1;
      }

      if (document.getElementById('tabSummary')?.classList.contains('active')) {
        loadHistory();
      }

      if (toast) {
        const ticketLabel = jiraTicket ? ` · ${jiraTicket}` : '';
        showToast(`Saved locally${ticketLabel} (cloud upload runs on a timer when logged in)`, 'success', 2500);
      }
      return true;
    }

    async function flushPendingActivity({ discardRemainder = false } = {}) {
      if (pendingActivitySeconds() < MIN_PERSISTABLE_ACTIVITY_SECONDS) {
        if (discardRemainder) pendingActivityMs = 0;
        return false;
      }
      if (!lastGoodSnapshot) {
        console.warn('[Tracking] No successful snapshot is available for pending time');
        if (discardRemainder) pendingActivityMs = 0;
        return false;
      }
      const { jiraTicket } = getTaskContext();
      const saved = await recordActivity(
        lastGoodSnapshot.description,
        lastGoodSnapshot.category,
        jiraTicket,
      );
      if (discardRemainder) pendingActivityMs = 0;
      return saved;
    }

    function scheduleCaptureRetry(runId) {
      if (!isMonitoring || runId !== captureRunId || captureRetryTimeout) return;
      captureRetryTimeout = setTimeout(() => {
        captureRetryTimeout = null;
        if (isMonitoring && runId === captureRunId) {
          captureAndAnalyze({ isRetry: true, runId });
        }
      }, CAPTURE_RETRY_DELAY_MS);
    }

    function scheduleMonitoringCaptures(runId) {
      if (monitoringInterval) clearInterval(monitoringInterval);
      clearCaptureRetry();
      captureAndAnalyze({ runId });
      monitoringInterval = setInterval(
        () => captureAndAnalyze({ runId }),
        captureIntervalMs,
      );
    }

    async function captureAndAnalyze({ isRetry = false, runId = captureRunId } = {}) {
      if (!isMonitoring || runId !== captureRunId) return;
      if (isSyncing) {
        console.log('[Sync] Already syncing, skipping this cycle');
        return;
      }

      isSyncing = true;

      const { task, jiraTicket } = getTaskContext();

      try {
        if (!isRetry) log('Taking Context Snapshot (background)...');

        const snapshot = await invoke('capture_context_snapshot', {
          userTask: task,
          jiraTicket: jiraTicket
        });

        if (!isMonitoring || runId !== captureRunId) {
          console.log('[Tracking] Ignoring snapshot from an inactive tracking run');
          return;
        }

        if (isAnalysisFailure(snapshot)) {
          console.warn('[Sync] Analysis failed — keeping elapsed time pending and retrying soon');
          scheduleCaptureRetry(runId);
          return;
        }

        lastGoodSnapshot = {
          description: snapshot.description,
          category: snapshot.category
        };

        log(`Snapshot ready: ${snapshot.description.substring(0, 50)}...`);
        if (!isMonitoring || runId !== captureRunId) return;
        await recordActivity(snapshot.description, snapshot.category, jiraTicket, {
          toast: !isRetry
        });
      } catch (e) {
        console.error('[Sync] Error:', e);
        log('Sync error: ' + e);
        if (isMonitoring && runId === captureRunId) {
          scheduleCaptureRetry(runId);
        }
      } finally {
        const activeRunId = captureRunId;
        const shouldCaptureActiveRun = isMonitoring && runId !== activeRunId;
        isSyncing = false;
        if (shouldCaptureActiveRun) {
          setTimeout(() => captureAndAnalyze({ runId: activeRunId }), 0);
        }
      }
    }

    // ===== TAB NAVIGATION =====
    const summaryBody = document.getElementById('summaryBody');
    let currentHistoryData = null;
    let currentWeekData = null;
    let lastStatusReportPayload = null;
    let reportProgressUnlisten = null;
    let reportProgressState = { activeStep: 0, doneThrough: -1, detail: '' };

    const REPORT_GEN_STEP_LABELS = [
      'Starting local AI engine…',
      'Section — project summary',
      'Section — overall workflow health',
      'Section — health breakdown table',
      'Section — timeline review',
      'Section — known issues',
      'Section — potential risks',
      'Section — progress & tasks completed',
      'Section — lessons & recommendations',
    ];

    function openStatusReportModal() {
      const modal = document.getElementById('statusReportModal');
      if (modal) {
        modal.classList.add('visible');
        modal.setAttribute('aria-hidden', 'false');
      }
    }

    function closeStatusReportModal() {
      const modal = document.getElementById('statusReportModal');
      if (modal) {
        modal.classList.remove('visible');
        modal.setAttribute('aria-hidden', 'true');
      }
    }

    document.getElementById('closeStatusReportModal')?.addEventListener('click', closeStatusReportModal);
    document.getElementById('statusReportModal')?.addEventListener('click', (e) => {
      if (e.target.id === 'statusReportModal') closeStatusReportModal();
    });
    const TASK_BAR_COLORS = [
      'hsl(var(--summary-purple))',
      'hsl(var(--summary-magenta))',
      'hsl(var(--summary-blue))',
      'hsl(var(--summary-teal))',
      'hsl(142 60% 45%)',
      'hsl(45 90% 50%)',
    ];
    const FOCUS_CATEGORIES = new Set(['Coding', 'Debugging', 'CodeReview', 'Testing', 'Design', 'DevOps', 'Database']);
    const DISTRACTION_CATEGORIES = new Set(['Browsing', 'Idle']);

    function switchTab(tabId) {
      document.querySelectorAll('.tab-panel').forEach(p => p.classList.remove('active'));
      document.querySelectorAll('.nav-item').forEach(n => n.classList.remove('active'));
      document.getElementById(tabId)?.classList.add('active');
      [...document.querySelectorAll('.nav-item')]
        .find((item) => item.dataset.tab === tabId)
        ?.classList.add('active');
      if (tabId === 'tabSummary') loadHistory();
      if (tabId === 'tabCloudInsights') loadCoachChat();
      if (tabId === 'tabToday') refreshTodayView();
    }

    document.querySelectorAll('.nav-item').forEach(btn => {
      btn.onclick = () => switchTab(btn.dataset.tab);
    });

    function formatDuration(totalSeconds) {
      totalSeconds = toFiniteNumber(totalSeconds, { min: 0, max: 86400, fallback: 0 });
      const hours = Math.floor(totalSeconds / 3600);
      const mins = Math.floor((totalSeconds % 3600) / 60);
      if (hours > 0) return `${hours}hr ${mins}min`;
      return `${mins}min`;
    }

    function formatShortDuration(totalSeconds) {
      totalSeconds = toFiniteNumber(totalSeconds, { min: 0, max: 86400, fallback: 0 });
      const hours = Math.floor(totalSeconds / 3600);
      const mins = Math.floor((totalSeconds % 3600) / 60);
      if (hours > 0) return `${hours}h ${mins}m`;
      return `${mins}m`;
    }

    function formatDisplayDate(dateStr) {
      const d = new Date(dateStr + 'T12:00:00');
      const today = new Date();
      const isToday = d.toDateString() === today.toDateString();
      const formatted = d.toLocaleDateString(undefined, { month: 'long', day: 'numeric' });
      if (isToday) return `Today, ${formatted}`;
      return formatted;
    }

    function buildTaskBreakdown(data) {
      const items = [];
      if (data.ticket_breakdown?.length) {
        data.ticket_breakdown.forEach(t => {
          items.push({ label: t.ticket, seconds: t.total_seconds });
        });
      }
      const unticketedMap = {};
      data.entries.forEach(e => {
        if (!e.ticket) {
          const label = e.category || 'General';
          unticketedMap[label] = (unticketedMap[label] || 0) + e.duration_seconds;
        }
      });
      Object.entries(unticketedMap).forEach(([label, seconds]) => {
        items.push({ label, seconds });
      });
      if (items.length === 0 && data.category_breakdown?.length) {
        data.category_breakdown.forEach(c => {
          items.push({ label: c.category, seconds: c.total_seconds });
        });
      }
      items.sort((a, b) => b.seconds - a.seconds);
      return items;
    }

    function computeFocusSeconds(data) {
      return data.category_breakdown
        .filter(c => FOCUS_CATEGORIES.has(c.category))
        .reduce((s, c) => s + c.total_seconds, 0);
    }

    function computeDistractionCount(data) {
      return data.entries.filter(e => DISTRACTION_CATEGORIES.has(e.category)).length;
    }

    function renderWeekRow(week) {
      if (!week?.days) return '';
      return week.days.map(day => {
        const classes = ['week-day'];
        if (day.is_today) classes.push('today');
        if (day.has_activity) classes.push('completed');
        const inner = day.has_activity
          ? '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3"><polyline points="20 6 9 17 4 12"/></svg>'
          : '';
        return `<div class="${classes.join(' ')}"><span class="week-day-label">${escapeHtml(day.weekday)}</span><div class="week-day-ring">${inner}</div></div>`;
      }).join('');
    }

    function renderProgressRing(progress) {
      const r = 30;
      const circ = 2 * Math.PI * r;
      const safeProgress = toFiniteNumber(progress, { min: 0, max: 1, fallback: 0 });
      const offset = circ * (1 - safeProgress);
      return `
        <div class="progress-ring">
          <svg viewBox="0 0 72 72">
            <circle class="progress-ring-bg" cx="36" cy="36" r="${r}"/>
            <circle class="progress-ring-fill" cx="36" cy="36" r="${r}"
              stroke-dasharray="${circ}" stroke-dashoffset="${offset}"/>
          </svg>
          <div class="progress-ring-icon">${renderIcon('hourglass', 22)}</div>
        </div>`;
    }

    function renderTaskBars(items, totalSeconds) {
      if (!items.length) return '<div class="empty-state" style="padding:12px;">No task time recorded yet</div>';
      const maxBar = Math.max(...items.map(i => i.seconds));
      return items.slice(0, 6).map((item, idx) => {
        const label = String(item.label ?? 'General');
        const pct = totalSeconds > 0 ? Math.round((item.seconds / totalSeconds) * 100) : 0;
        const barWidth = maxBar > 0 ? Math.round((item.seconds / maxBar) * 100) : 0;
        const fillWidth = item.seconds > 0 ? Math.max(barWidth, 4) : 0;
        const minInsidePct = Math.min(32, 10 + label.length * 3.5);
        const labelOutside = fillWidth < minInsidePct;
        const color = TASK_BAR_COLORS[idx % TASK_BAR_COLORS.length];
        const mins = Math.round(item.seconds / 60);
        const labelClass = labelOutside ? 'task-bar-label task-bar-label-outside' : 'task-bar-label task-bar-label-inside';
        const labelStyle = labelOutside
          ? `left:calc(${fillWidth}% + 6px);max-width:calc(${100 - fillWidth}% - 10px);`
          : `max-width:calc(${fillWidth}% - 16px);`;
        return `
          <div class="task-bar-row">
            <span class="task-bar-pct">${pct}%</span>
            <div class="task-bar-track">
              <div class="task-bar-fill" style="width:${fillWidth}%;background:${color}"></div>
              <span class="${labelClass}" style="${labelStyle}">${escapeHtml(label)}</span>
            </div>
            <span class="task-bar-time">${mins}m</span>
          </div>`;
      }).join('');
    }

    function renderFocusChart(entries) {
      const focusEntries = entries.filter(e => FOCUS_CATEGORIES.has(e.category));
      if (!focusEntries.length) {
        return '<div class="focus-chart-empty">No deep focus sessions yet today</div>';
      }

      const byHour = new Array(24).fill(0);
      focusEntries.forEach(e => {
        const hour = new Date(e.time).getHours();
        if (Number.isInteger(hour) && hour >= 0 && hour < 24) {
          byHour[hour] += toFiniteNumber(e.duration_seconds, { min: 0, max: 86400, fallback: 0 });
        }
      });

      const startHour = 8;
      const endHour = 20;
      const slots = [];
      for (let h = startHour; h < endHour; h++) {
        slots.push({ hour: h, seconds: byHour[h] });
      }

      const maxSec = Math.max(...slots.map(s => s.seconds), 1);
      const bars = slots.map(({ hour, seconds }) => {
        const pct = seconds > 0 ? Math.max(Math.round((seconds / maxSec) * 100), 12) : 8;
        const mins = Math.round(seconds / 60);
        const tip = mins > 0 ? `${hour}:00 — ${mins} min focus` : `${hour}:00 — no focus`;
        const cls = seconds > 0 ? 'focus-bar' : 'focus-bar focus-bar-empty';
        return `<div class="${cls}" style="height:${pct}%" title="${escapeHtml(tip)}"></div>`;
      }).join('');

      return `
        <div class="focus-chart-wrap">
          <div class="focus-chart">${bars}</div>
          <div class="focus-chart-axis"><span>8am</span><span>8pm</span></div>
          <div class="focus-chart-caption">Deep focus minutes per hour · hover a bar for details</div>
        </div>`;
    }

    function renderTimeline(entries) {
      if (!entries.length) return '';

      function formatDesc(text) {
        return escapeHtml(text)
          .replace(/APP:/g, '<br><strong>APP:</strong>')
          .replace(/WINDOW TITLE:/g, '<br><strong>WINDOW TITLE:</strong>')
          .replace(/VISIBLE CONTENT:/g, '<br><strong>VISIBLE CONTENT:</strong>')
          .replace(/FILES OR URLS:/g, '<br><strong>FILES OR URLS:</strong>')
          .replace(/CURRENT ACTION:/g, '<br><strong>CURRENT ACTION:</strong>')
          .replace(/PROGRESS:/g, '<br><strong>PROGRESS:</strong>')
          .replace(/NEXT STEP:/g, '<br><strong>NEXT STEP:</strong>')
          .replace(/CATEGORY:/g, '<br><strong>CATEGORY:</strong>')
          .replace(/^<br>/, '');
      }

      return `
        <div class="timeline-section">
          <div class="timeline-section-title">Timeline</div>
          ${entries.map(e => {
            const time = new Date(e.time).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
            const durationSeconds = Math.max(0, Number(e.duration_seconds) || 0);
            const dur = durationSeconds < 120
              ? Math.round(durationSeconds) + 's'
              : Math.round(durationSeconds / 60) + 'm';
            const ticket = e.ticket ? `<span class="badge badge-success" style="font-size:10px;margin-left:6px;">${escapeHtml(e.ticket)}</span>` : '';
            return `
              <div class="timeline-item">
                <div style="flex:1;">
                  <div style="font-size:11px;color:hsl(var(--muted-foreground));display:flex;justify-content:space-between;">
                    <span>${escapeHtml(time)}</span><span>${dur}</span>
                  </div>
                  <div style="font-size:12px;font-weight:500;margin-top:2px;">${escapeHtml(e.category)}${ticket}</div>
                  <div style="font-size:11px;color:hsl(var(--foreground));opacity:0.8;line-height:1.6;">${formatDesc(e.description)}</div>
                </div>
              </div>`;
          }).join('')}
        </div>`;
    }

    function renderHistory(data, week) {
      if (data.entries.length === 0) {
        summaryBody.innerHTML = `
          <div class="summary-header">
            <h1 class="summary-title">Summary</h1>
            <button class="summary-download-btn" id="generateReportBtn" title="Generate AI work report" aria-label="Generate AI work report">
              <span class="summary-download-label">Work report</span>
              <span class="summary-download-icon">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="4" width="18" height="18" rx="2"/><line x1="16" y1="2" x2="16" y2="6"/><line x1="8" y1="2" x2="8" y2="6"/><line x1="3" y1="10" x2="21" y2="10"/></svg>
              </span>
            </button>
          </div>
          <div class="summary-date">${escapeHtml(formatDisplayDate(data.date))}</div>
          <div class="week-row">${renderWeekRow(week)}</div>
          <div class="empty-state">No activity recorded today</div>`;
        document.getElementById('generateReportBtn').onclick = () => {
          generateStatusReport().catch((e) => showToast(String(e), 'error', 6000));
        };
        return;
      }

      const goalSeconds = getDailyGoalHours() * 3600;
      const progress = data.total_seconds / goalSeconds;
      const goalMet = data.total_seconds >= goalSeconds;
      const taskItems = buildTaskBreakdown(data);
      const focusSeconds = computeFocusSeconds(data);
      const distractionCount = computeDistractionCount(data);
      const yesterdayTotal = week?.yesterday_seconds || 0;
      let focusInsight = 'Start tracking to see your focus insights.';
      if (yesterdayTotal > 0) {
        const diff = Math.round(((data.total_seconds - yesterdayTotal) / yesterdayTotal) * 100);
        if (diff > 0) focusInsight = `Your tracked time is <strong>${diff}% higher</strong> than yesterday. Keep going!`;
        else if (diff < 0) focusInsight = `Your tracked time is <strong>${Math.abs(diff)}% lower</strong> than yesterday.`;
        else focusInsight = 'Your tracked time matches yesterday.';
      } else if (focusSeconds > 0) {
        focusInsight = `You've logged <strong>${formatShortDuration(focusSeconds)}</strong> of deep focus today.`;
      }

      summaryBody.innerHTML = `
        <div class="summary-header">
          <h1 class="summary-title">Summary</h1>
          <button class="summary-download-btn" id="generateReportBtn" title="Generate AI work report" aria-label="Generate AI work report">
            <span class="summary-download-label">Work report</span>
            <span class="summary-download-icon">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="4" width="18" height="18" rx="2"/><line x1="16" y1="2" x2="16" y2="6"/><line x1="8" y1="2" x2="8" y2="6"/><line x1="3" y1="10" x2="21" y2="10"/></svg>
            </span>
          </button>
        </div>
        <div class="summary-date">${escapeHtml(formatDisplayDate(data.date))}</div>
        <div class="week-row">${renderWeekRow(week)}</div>

        <div class="summary-main-card">
          <div class="summary-time-row">
            ${renderProgressRing(progress)}
            <div>
              <div class="summary-total-time">${formatDuration(data.total_seconds)}</div>
              <div class="summary-goal">
                <span class="goal-dot ${goalMet ? '' : 'behind'}"></span>
                Daily goal ${escapeHtml(getDailyGoalHours())} hours
              </div>
            </div>
          </div>
          ${renderTaskBars(taskItems, data.total_seconds)}
        </div>

        <div class="highlights-title">Highlights</div>

        <div class="highlight-card highlight-focus">
          <div class="highlight-card-header">${renderIcon('target', 15)} Deep Focus</div>
          <div class="highlight-insight">${focusInsight}</div>
          ${renderFocusChart(data.entries)}
        </div>

        <div class="highlight-card highlight-distraction">
          <div class="distraction-row">
            <div>
              <div class="highlight-card-header">${renderIcon('smartphone', 15)} Distractions today</div>
            </div>
            <div class="distraction-count">${distractionCount}</div>
          </div>
        </div>

        ${renderTimeline(data.entries)}`;

      document.getElementById('generateReportBtn').onclick = () => {
        generateStatusReport().catch((e) => showToast(String(e), 'error', 6000));
      };
    }

    function healthStatusClass(status) {
      const s = String(status ?? '').toLowerCase();
      if (s.includes('risk')) return 'sr-health-at-risk';
      if (s.includes('attention')) return 'sr-health-attention';
      return 'sr-health-on-track';
    }

    function renderReportGeneratingUI() {
      openStatusReportModal();
      const host = document.getElementById('statusReportModalBody');
      if (!host) return;

      const { activeStep, doneThrough, detail } = reportProgressState;

      host.innerHTML = `
        <div class="sr-generating">
          <div class="sr-gen-spinner"></div>
          <div class="sr-gen-title">Building your work report</div>
          <div class="sr-gen-steps" id="srGenSteps">
            ${REPORT_GEN_STEP_LABELS.map((label, i) => {
              const isDone = i <= doneThrough;
              const isActive = i === activeStep && !isDone;
              const icon = isDone ? '✓' : isActive ? '○' : '○';
              const stepDetail = isActive && detail
                ? `<span class="sr-gen-detail">${escapeHtml(extractEnglishText(detail))}</span>`
                : isDone && i === doneThrough && detail && i > 0
                  ? `<span class="sr-gen-detail">${escapeHtml(extractEnglishText(detail).slice(0, 120))}</span>`
                  : '';
              return `
              <div class="sr-gen-step ${isActive ? 'active' : isDone ? 'done' : ''}" data-step="${i}">
                <span>${icon} ${escapeHtml(label)}</span>
                ${stepDetail}
              </div>`;
            }).join('')}
          </div>
          <div class="sr-gen-progress-text">
            Step ${Math.min(doneThrough + 2, REPORT_GEN_STEP_LABELS.length)} of ${REPORT_GEN_STEP_LABELS.length}
          </div>
        </div>`;
    }

    function handleReportProgress(payload) {
      const step = Math.round(toFiniteNumber(
        payload?.step,
        { min: 0, max: REPORT_GEN_STEP_LABELS.length - 1, fallback: 0 },
      ));
      const phase = payload?.phase || 'start';
      const detail = payload?.detail || payload?.label || '';

      if (phase === 'start') {
        reportProgressState.activeStep = step;
        reportProgressState.detail = detail;
      } else if (phase === 'done') {
        reportProgressState.doneThrough = Math.max(reportProgressState.doneThrough, step);
        reportProgressState.activeStep = Math.min(step + 1, REPORT_GEN_STEP_LABELS.length - 1);
        reportProgressState.detail = detail;
      }

      renderReportGeneratingUI();
    }

    async function beginReportProgressListen() {
      if (reportProgressUnlisten) {
        safeUnlisten(reportProgressUnlisten);
        reportProgressUnlisten = null;
      }
      reportProgressUnlisten = await listen('local-report-progress', (event) => {
        handleReportProgress(event.payload);
      });
    }

    async function endReportProgressListen() {
      if (reportProgressUnlisten) {
        safeUnlisten(reportProgressUnlisten);
        reportProgressUnlisten = null;
      }
    }

    function renderReportCategoryBars(local) {
      const cats = (local.category_breakdown || []).slice(0, 5);
      if (!cats.length) return '';
      const maxSec = Math.max(...cats.map((c) => toFiniteNumber(c.total_seconds, { min: 0, max: 604800 })), 1);
      return `
        <div class="sr-tbi-cat-bars">
          ${cats.map((c, i) => {
            const seconds = toFiniteNumber(c.total_seconds, { min: 0, max: 604800 });
            const pct = Math.max(4, Math.round((seconds / maxSec) * 100));
            const color = TASK_BAR_COLORS[i % TASK_BAR_COLORS.length];
            return `
              <div class="sr-tbi-cat-bar-row">
                <span>${escapeHtml(c.category)}</span>
                <div class="sr-tbi-cat-bar-track">
                  <div class="sr-tbi-cat-bar-fill" style="width:${pct}%;background:${color}"></div>
                </div>
                <span>${(seconds / 3600).toFixed(1)}h</span>
              </div>`;
          }).join('')}
        </div>`;
    }

    function renderStatusReportPanel(payload) {
      openStatusReportModal();
      const host = document.getElementById('statusReportModalBody');
      if (!host || !payload) return;

      const report = payload.report || {};
      const meta = report.report_meta || {};
      const local = payload.local_data || {};
      const userName = currentUser?.display_name || currentUser?.name || currentUser?.email || 'Developer';
      const period = meta.period_label || (local.period_start && local.period_end
        ? `${local.period_start} → ${local.period_end}`
        : '');
      const healthClass = healthStatusClass(report.overall_health);
      const focusPct = local.total_seconds > 0
        ? Math.round((local.focus_seconds / local.total_seconds) * 100)
        : 0;
      const generatedDate = payload.generated_at || new Date().toLocaleDateString(undefined, {
        month: 'long', day: 'numeric', year: 'numeric',
      });

      const breakdownHtml = (report.health_breakdown || []).map((row, idx) => `
        <div class="sr-tbi-table-row" style="${idx === 0 ? '' : ''}">
          <span>${enReport(row.element)}</span>
          <span class="sr-status-pill ${healthStatusClass(row.status)}">${enReport(row.status)}</span>
          <span>${enReport(row.owner_team || 'Self')}</span>
          <span>${enReport(row.notes)}</span>
        </div>`).join('');

      const listItems = (items) => (items || []).map((i) => `<li>${enReport(i)}</li>`).join('');

      const lessonsHtml = (report.lessons_learned || []).map((l) => `
        <div class="sr-lesson-card">
          <div class="sr-lesson-title">${enReport(l.title)}</div>
          <p class="sr-lesson-body">${enReport(l.body)}</p>
        </div>`).join('');

      const passesHtml = (payload.generation_passes || []).map((p) => `
        <span class="sr-pass-chip done" title="${escapeHtml(p.detail || '')}">${escapeHtml((p.label || p.id || '').replace(/^Section — /, ''))}</span>`).join('');

      const weekDays = currentWeekData?.days || [];
      const weekMax = Math.max(...weekDays.map((d) => toFiniteNumber(d.total_seconds, { min: 0, max: 86400 })), 1);
      const weekStrip = weekDays.length ? `
        <div class="sr-week-strip">
          ${weekDays.map((d) => {
            const seconds = toFiniteNumber(d.total_seconds, { min: 0, max: 86400 });
            const pct = Math.max(8, Math.round((seconds / weekMax) * 100));
            return `<div class="sr-week-bar" title="${escapeHtml(`${d.date}: ${(seconds / 3600).toFixed(1)}h`)}">
              <div class="sr-week-bar-fill" style="height:${pct}%"></div>
              <span class="sr-week-bar-label">${escapeHtml(d.weekday)}</span>
            </div>`;
          }).join('')}
        </div>` : '';

      host.innerHTML = `
        <div class="status-report status-report-tbi">
          <div class="sr-tbi-title-block">
            <h2 class="sr-tbi-title">Work Status Report</h2>
            <div class="sr-tbi-subline">Flowmates · ${escapeHtml(generatedDate)} · ${escapeHtml(userName)}</div>
          </div>

          <div class="sr-tbi-meta-row">
            <div class="sr-tbi-meta-col">
              <div class="sr-tbi-meta-box">
                <label>Period</label>
                <span>${escapeHtml(period)}</span>
              </div>
              <div class="sr-tbi-meta-box">
                <label>Focus target</label>
                <span>${enReport(report.focus_target || meta.focus_target || '—')}</span>
              </div>
              <div class="sr-tbi-meta-box">
                <label>Tracked hours</label>
                <span>${escapeHtml(local.total_hours ?? 0)}h · ${focusPct}% focus</span>
              </div>
            </div>
            <div class="sr-tbi-summary-box">
              <p>${enReport(report.executive_overview || report.work_summary || '')}</p>
            </div>
          </div>

          <div>
            <h3 class="sr-tbi-section-title">Overall project health</h3>
            <div class="sr-tbi-health-row">
              <div class="sr-tbi-health-badge ${healthClass}">${enReport(report.overall_health || 'Attention')}</div>
              <p class="sr-tbi-health-notes">${enReport(report.health_notes || '')}</p>
            </div>
          </div>

          <div>
            <h3 class="sr-tbi-section-title">Project health breakdown</h3>
            <div class="sr-tbi-table">
              <div class="sr-tbi-table-head">
                <span>Element</span><span>Status</span><span>Owner/team</span><span>Notes</span>
              </div>
              ${breakdownHtml || '<div class="sr-tbi-table-row"><span colspan="4">No breakdown available.</span></div>'}
            </div>
          </div>

          <div>
            <h3 class="sr-tbi-section-title">Timeline review</h3>
            <div class="sr-tbi-timeline">
              ${weekStrip}
              ${renderReportCategoryBars(local)}
              <p class="sr-tbi-timeline-caption">${enReport(report.timeline_caption || report.work_summary || '')}</p>
            </div>
          </div>

          <div class="sr-tbi-footer">
            <div>
              <div class="sr-tbi-risk-block" style="margin-bottom:8px;">
                <h4 class="sr-tbi-block-title">💡 Known issues</h4>
                <ul>${listItems(report.known_issues) || '<li>None flagged</li>'}</ul>
              </div>
              <div class="sr-tbi-risk-block">
                <h4 class="sr-tbi-block-title">⚠ Potential risks</h4>
                <ul>${listItems(report.potential_risks) || '<li>None flagged</li>'}</ul>
              </div>
            </div>
            <div class="sr-tbi-progress-block">
              <h4 class="sr-tbi-block-title">Team progress</h4>
              <p style="font-size:10px;font-weight:700;margin:0 0 6px;">Tasks completed</p>
              <ul>${listItems(report.tasks_completed) || '<li>No tasks listed</li>'}</ul>
              <p style="font-size:10px;font-weight:700;margin:12px 0 6px;">Highlights</p>
              <ul>${listItems(report.work_progress) || '<li>No highlights</li>'}</ul>
            </div>
          </div>

          <div class="sr-tbi-lessons">
            <h3 class="sr-tbi-section-title">Lessons learned</h3>
            ${lessonsHtml || '<p style="font-size:11px;color:hsl(var(--muted-foreground))">No lessons generated.</p>'}
          </div>

          ${(report.recommendations || []).length ? `
          <div class="sr-card sr-recs">
            <h3>Recommendations</h3>
            <ul>${listItems(report.recommendations)}</ul>
          </div>` : ''}

          <div class="sr-footer">
            <div class="sr-passes">
              ${passesHtml}
              ${payload.ai_powered ? '' : '<span class="sr-pass-chip">Basic mode</span>'}
            </div>
            <button type="button" class="sr-download-btn" id="downloadReportPdfBtn">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
              Download PDF
            </button>
          </div>
        </div>`;

      document.getElementById('downloadReportPdfBtn')?.addEventListener('click', () => {
        exportStatusReportPdf(payload).catch((e) => showToast('PDF export failed: ' + e, 'error'));
      });
    }

    async function generateStatusReport() {
      const epoch = accountEpoch;
      const accountId = currentAccountId;

      const btn = document.getElementById('generateReportBtn');
      const labelEl = btn?.querySelector('.summary-download-label');
      const originalLabel = labelEl?.textContent;

      if (btn) {
        btn.disabled = true;
        if (labelEl) labelEl.textContent = 'Generating…';
      }

      openStatusReportModal();
      reportProgressState = { activeStep: 0, doneThrough: -1, detail: 'Connecting…' };
      renderReportGeneratingUI();
      let localReportUnlistenDone = null;
      let localReportUnlistenError = null;
      let reportTimeoutId = null;

      try {
        await beginReportProgressListen();

        let resolveReport;
        let rejectReport;
        const reportResultPromise = new Promise((resolve, reject) => {
          resolveReport = resolve;
          rejectReport = reject;
        });
        localReportUnlistenDone = await listen('local-report-done', (event) => {
          if (!guard(epoch, accountId)) return;
          resolveReport(event.payload);
        });
        localReportUnlistenError = await listen('local-report-error', (event) => {
          if (!guard(epoch, accountId)) return;
          rejectReport(new Error(event.payload?.error || 'Report generation failed'));
        });
        const reportTimeoutPromise = new Promise((_, reject) => {
          reportTimeoutId = setTimeout(
            () => reject(new Error('Report generation timed out after 45s')),
            45_000,
          );
        });

        const res = await invoke('generate_local_status_report', { periodDays: 7 });
        if (!guard(epoch, accountId)) return;
        if (res.status !== 'started') {
          throw new Error('Unexpected response: ' + JSON.stringify(res));
        }

        const payload = await Promise.race([reportResultPromise, reportTimeoutPromise]);
        if (!guard(epoch, accountId)) return;
        lastStatusReportPayload = payload;
        reportProgressState = {
          activeStep: REPORT_GEN_STEP_LABELS.length - 1,
          doneThrough: REPORT_GEN_STEP_LABELS.length - 1,
          detail: 'Complete',
        };
        renderReportGeneratingUI();
        await new Promise((r) => setTimeout(r, 400));
        if (!guard(epoch, accountId)) return;
        renderStatusReportPanel(payload);
        showToast(payload.ai_powered ? 'Work report ready' : 'Basic report ready (AI unavailable)', 'success');
      } catch (e) {
        const host = document.getElementById('statusReportModalBody');
        if (host) {
          host.innerHTML = `<div class="empty-state" style="color:hsl(var(--destructive))">Report failed: ${escapeHtml(String(e))}</div>`;
        }
        throw e;
      } finally {
        if (reportTimeoutId) clearTimeout(reportTimeoutId);
        safeUnlisten(localReportUnlistenDone);
        safeUnlisten(localReportUnlistenError);
        await endReportProgressListen().catch(() => {});
        if (btn) {
          btn.disabled = false;
          if (labelEl && originalLabel) labelEl.textContent = originalLabel;
        }
      }
    }

    function buildStatusReportPdfFilename(payload, todayData) {
      const local = payload?.local_data || {};
      const isoDay = (value) => {
        const s = String(value ?? '').trim();
        const m = s.match(/(\d{4}-\d{2}-\d{2})/);
        return m ? m[1] : '';
      };
      const start = isoDay(local.period_start);
      const end = isoDay(local.period_end) || isoDay(todayData?.date) || new Date().toISOString().slice(0, 10);
      if (start && end && start !== end) {
        return `Flowmates_Work-Report_${start}_to_${end}.pdf`;
      }
      return `Flowmates_Work-Report_${end || start}.pdf`;
    }

    function showPdfDownloadModal(savedPath, filename) {
      if (document.getElementById('pdfDownloadOverlay')) return;

      const overlay = document.createElement('div');
      overlay.className = 'modal-overlay';
      overlay.id = 'pdfDownloadOverlay';
      overlay.style.zIndex = '2100';

      const folderPath = savedPath.replace(/[/\\][^/\\]+$/, '') || savedPath;
      const displayName = filename || savedPath.split(/[/\\]/).pop() || 'report.pdf';

      overlay.innerHTML = `
        <div class="modal-content" style="max-width:420px;">
          <div class="modal-header">
            <span class="modal-title">Report downloaded</span>
          </div>
          <div class="modal-body">
            <p style="font-size:13px;margin:0 0 4px;">Your PDF is ready in your Downloads folder.</p>
            <div class="pdf-download-path">
              <div class="pdf-download-filename">${escapeHtml(displayName)}</div>
              <div style="color:hsl(var(--muted-foreground));">${escapeHtml(folderPath)}</div>
            </div>
            <div style="display:flex;gap:8px;margin-top:16px;justify-content:flex-end;flex-wrap:wrap;">
              <button type="button" id="pdfDownloadOpenFolderBtn" style="padding:6px 14px;font-size:12px;font-weight:500;border-radius:var(--radius);cursor:pointer;border:1px solid hsl(var(--border));background:hsl(var(--secondary));color:hsl(var(--secondary-foreground));">Open folder</button>
              <button type="button" id="pdfDownloadOkBtn" style="padding:6px 14px;font-size:12px;font-weight:500;border-radius:var(--radius);cursor:pointer;border:none;background:hsl(var(--primary));color:hsl(var(--primary-foreground));">OK</button>
            </div>
          </div>
        </div>`;

      document.body.appendChild(overlay);
      const close = () => overlay.remove();
      overlay.querySelector('#pdfDownloadOkBtn')?.addEventListener('click', close);
      overlay.addEventListener('click', (e) => {
        if (e.target === overlay) close();
      });
      overlay.querySelector('#pdfDownloadOpenFolderBtn')?.addEventListener('click', async () => {
        try {
          await invoke('open_path_in_file_manager', { path: savedPath });
        } catch (err) {
          showToast('Could not open folder: ' + err, 'error');
        }
      });
    }

    async function exportStatusReportPdf(payload) {
      if (!payload) payload = lastStatusReportPayload;
      if (!payload) {
        showToast('Generate a report first', 'error');
        return;
      }
      const { doc, filename } = buildStatusReportPdf(payload, currentHistoryData, currentWeekData);
      const bytes = Array.from(new Uint8Array(doc.output('arraybuffer')));
      const savedPath = await invoke('save_pdf_to_downloads', { filename, bytes });
      showPdfDownloadModal(savedPath, filename);
    }

    function renderBreakdown(items, typeKey) {
      if (!items || items.length === 0) return '';
      items.sort((a, b) => b.total_seconds - a.total_seconds);
      const max = items[0].total_seconds;
      return items.map(item => {
        const label = typeKey === 'category' ? item.category : item.ticket;
        const seconds = toFiniteNumber(item.total_seconds, { min: 0, max: 86400 });
        const safeMax = Math.max(1, toFiniteNumber(max, { min: 0, max: 86400 }));
        const percent = Math.round((seconds / safeMax) * 100);
        const mins = Math.round(seconds / 60);
        return `
          <div class="breakdown-item">
            <div style="flex: 1;">
              <div style="display: flex; justify-content: space-between; margin-bottom: 2px;">
                <span>${escapeHtml(label)}</span>
                <span>${mins} min</span>
              </div>
              <div class="breakdown-bar">
                <div class="breakdown-fill" style="width: ${percent}%"></div>
              </div>
            </div>
          </div>`;
      }).join('');
    }

    async function loadHistory() {
      summaryBody.innerHTML = '<div class="empty-state"><span class="loading"></span> Loading summary...</div>';
      const epoch = accountEpoch;
      const accountId = currentAccountId;
      try {
        const [history, week] = await Promise.all([
          invoke('get_today_history'),
          invoke('get_week_summary').catch(() => null),
        ]);
        if (!guard(epoch, accountId)) return;
        currentHistoryData = history;
        currentWeekData = week;
        renderHistory(history, week);
      } catch (e) {
        summaryBody.innerHTML = `<div class="empty-state" style="color:hsl(var(--destructive))">Failed to load summary: ${escapeHtml(String(e))}</div>`;
      }
    }

    const PDF_MARGIN = 16;
    const PDF_PAGE_W = 210;
    const PDF_CONTENT_W = PDF_PAGE_W - PDF_MARGIN * 2;

    function pdfEnsureSpace(doc, y, needed = 20) {
      if (y + needed > 282) {
        doc.addPage();
        return PDF_MARGIN;
      }
      return y;
    }

    function pdfWrapText(doc, text, x, y, maxWidth, lineHeight = 4.8) {
      const lines = doc.splitTextToSize(String(text ?? ''), maxWidth);
      doc.text(lines, x, y);
      return y + lines.length * lineHeight;
    }

    function pdfHealthColor(status) {
      const s = String(status ?? '').toLowerCase();
      if (s.includes('risk')) return [229, 115, 115];
      if (s.includes('attention')) return [255, 213, 79];
      return [77, 182, 172];
    }

    function pdfHealthTextColor(status) {
      const s = String(status ?? '').toLowerCase();
      if (s.includes('attention')) return [93, 64, 55];
      return [255, 255, 255];
    }

    function pdfSectionLabel(doc, title, y) {
      y = pdfEnsureSpace(doc, y, 12);
      doc.setFont(undefined, 'bold');
      doc.setFontSize(11);
      doc.setTextColor(35, 35, 40);
      doc.text(String(title), PDF_MARGIN, y);
      return y + 7;
    }

    function pdfDrawTbiTitle(doc, y, userName, generatedAt) {
      y = pdfEnsureSpace(doc, y, 22);
      doc.setFont(undefined, 'bold');
      doc.setFontSize(18);
      doc.setTextColor(30, 30, 35);
      doc.text('Work Status Report', PDF_PAGE_W / 2, y, { align: 'center' });
      y += 8;
      doc.setDrawColor(210, 210, 215);
      doc.line(PDF_MARGIN, y, PDF_PAGE_W - PDF_MARGIN, y);
      y += 6;
      doc.setFont(undefined, 'normal');
      doc.setFontSize(9);
      doc.setTextColor(100, 100, 110);
      doc.text(`Flowmates · ${generatedAt} · ${userName}`, PDF_PAGE_W / 2, y, { align: 'center' });
      return y + 10;
    }

    function pdfDrawMetaSummaryRow(doc, y, metaBoxes, summaryText) {
      y = pdfEnsureSpace(doc, y, 42);
      const leftW = 62;
      const gap = 6;
      const rightX = PDF_MARGIN + leftW + gap;
      const rightW = PDF_CONTENT_W - leftW - gap;
      const boxH = 11;
      let leftY = y;

      metaBoxes.forEach((box) => {
        doc.setFillColor(245, 245, 248);
        doc.setDrawColor(225, 225, 232);
        doc.roundedRect(PDF_MARGIN, leftY, leftW, boxH, 2, 2, 'FD');
        doc.setFont(undefined, 'bold');
        doc.setFontSize(6.5);
        doc.setTextColor(120, 120, 130);
        doc.text(box.label.toUpperCase(), PDF_MARGIN + 4, leftY + 4);
        doc.setFont(undefined, 'normal');
        doc.setFontSize(8);
        doc.setTextColor(40, 40, 48);
        doc.text(String(box.value).slice(0, 28), PDF_MARGIN + 4, leftY + 8.5);
        leftY += boxH + 3;
      });

      const summaryH = Math.max(38, leftY - y);
      doc.setFillColor(227, 242, 253);
      doc.setDrawColor(179, 229, 252);
      doc.roundedRect(rightX, y, rightW, summaryH, 3, 3, 'FD');
      doc.setFont(undefined, 'normal');
      doc.setFontSize(9);
      doc.setTextColor(45, 55, 72);
      pdfWrapText(doc, summaryText, rightX + 5, y + 7, rightW - 10, 4.3);
      return y + summaryH + 8;
    }

    function pdfDrawOverallHealth(doc, y, status, notes) {
      y = pdfSectionLabel(doc, 'Overall project health:', y);
      y = pdfEnsureSpace(doc, y, 28);
      const badgeW = 38;
      const badgeH = 22;
      const [r, g, b] = pdfHealthColor(status);
      const [tr, tg, tb] = pdfHealthTextColor(status);
      doc.setFillColor(r, g, b);
      doc.roundedRect(PDF_MARGIN, y, badgeW, badgeH, 3, 3, 'F');
      doc.setFont(undefined, 'bold');
      doc.setFontSize(10);
      doc.setTextColor(tr, tg, tb);
      doc.text(String(status || 'Attention'), PDF_MARGIN + badgeW / 2, y + 13, { align: 'center' });
      doc.setFont(undefined, 'normal');
      doc.setFontSize(9);
      doc.setTextColor(55, 55, 65);
      const notesEnd = pdfWrapText(doc, notes, PDF_MARGIN + badgeW + 8, y + 5, PDF_CONTENT_W - badgeW - 8, 4.4);
      return Math.max(y + badgeH, notesEnd) + 8;
    }

    function pdfDrawTbiTable(doc, y, rows) {
      y = pdfSectionLabel(doc, 'Project health breakdown:', y);
      y = pdfEnsureSpace(doc, y, 14);
      const cols = { element: PDF_MARGIN + 2, status: 52, owner: 78, notes: 108 };
      doc.setFillColor(55, 58, 64);
      doc.roundedRect(PDF_MARGIN, y - 4, PDF_CONTENT_W, 7, 1, 1, 'F');
      doc.setFont(undefined, 'bold');
      doc.setFontSize(7);
      doc.setTextColor(255, 255, 255);
      doc.text('Element', cols.element, y);
      doc.text('Status', cols.status, y);
      doc.text('Owner/team', cols.owner, y);
      doc.text('Notes', cols.notes, y);
      y += 6;

      rows.forEach((row, idx) => {
        y = pdfEnsureSpace(doc, y, 10);
        if (idx % 2 === 0) {
          doc.setFillColor(248, 248, 250);
          doc.rect(PDF_MARGIN, y - 3.5, PDF_CONTENT_W, 8, 'F');
        }
        doc.setFont(undefined, 'normal');
        doc.setFontSize(7.5);
        doc.setTextColor(40, 40, 48);
        doc.text(String(row.element ?? '').slice(0, 16), cols.element, y);
        const [r, g, b] = pdfHealthColor(row.status);
        const [tr, tg, tb] = pdfHealthTextColor(row.status);
        doc.setFillColor(r, g, b);
        doc.roundedRect(cols.status, y - 3.2, 22, 5.5, 1, 1, 'F');
        doc.setFont(undefined, 'bold');
        doc.setFontSize(6);
        doc.setTextColor(tr, tg, tb);
        doc.text(String(row.status ?? '').slice(0, 10), cols.status + 11, y, { align: 'center' });
        doc.setFont(undefined, 'normal');
        doc.setFontSize(7.5);
        doc.setTextColor(70, 70, 80);
        doc.text(String(row.owner_team ?? 'Self').slice(0, 12), cols.owner, y);
        const notesY = pdfWrapText(doc, row.notes ?? '', cols.notes, y, PDF_PAGE_W - cols.notes - PDF_MARGIN, 3.8);
        y = Math.max(y + 5, notesY) + 2;
      });
      return y + 4;
    }

    function pdfDrawTimelineReview(doc, y, weekDays, categories, caption) {
      y = pdfSectionLabel(doc, 'Timeline review:', y);
      y = pdfDrawWeekChart(doc, y, weekDays);
      if (categories.length) {
        const catMax = Math.max(...categories.map((c) => c.value), 1);
        y = pdfDrawBarChart(doc, y, 'Time by category', categories, catMax);
      }
      doc.setFont(undefined, 'normal');
      doc.setFontSize(8.5);
      doc.setTextColor(90, 90, 100);
      y = pdfWrapText(doc, caption, PDF_MARGIN, y, PDF_CONTENT_W, 4.2) + 6;
      return y;
    }

    function pdfDrawRisksProgressColumns(doc, y, knownIssues, potentialRisks, tasksCompleted, workProgress) {
      y = pdfEnsureSpace(doc, y, 50);
      const colW = (PDF_CONTENT_W - 6) / 2;
      const leftX = PDF_MARGIN;
      const rightX = PDF_MARGIN + colW + 6;
      const blockH = 46;

      doc.setFillColor(255, 251, 235);
      doc.roundedRect(leftX, y, colW, blockH, 2, 2, 'F');
      doc.setFillColor(227, 242, 253);
      doc.roundedRect(rightX, y, colW, blockH, 2, 2, 'F');

      doc.setFont(undefined, 'bold');
      doc.setFontSize(8);
      doc.setTextColor(120, 80, 20);
      doc.text('Known issues', leftX + 4, y + 6);
      doc.setTextColor(40, 90, 130);
      doc.text('Team progress', rightX + 4, y + 6);

      doc.setFont(undefined, 'normal');
      doc.setFontSize(7.2);
      doc.setTextColor(60, 60, 70);
      let ly = y + 11;
      (knownIssues || []).slice(0, 3).forEach((item) => {
        ly = pdfWrapText(doc, `• ${item}`, leftX + 4, ly, colW - 8, 3.6) + 1;
      });
      doc.setFont(undefined, 'bold');
      doc.setFontSize(7);
      doc.setTextColor(40, 90, 130);
      doc.text('Potential risks', leftX + 4, y + 28);
      doc.setFont(undefined, 'normal');
      doc.setTextColor(60, 60, 70);
      ly = y + 33;
      (potentialRisks || []).slice(0, 2).forEach((item) => {
        ly = pdfWrapText(doc, `• ${item}`, leftX + 4, ly, colW - 8, 3.6) + 1;
      });

      doc.setFont(undefined, 'bold');
      doc.setFontSize(7);
      doc.setTextColor(40, 90, 130);
      doc.text('Tasks completed', rightX + 4, y + 12);
      doc.setFont(undefined, 'normal');
      let ry = y + 17;
      (tasksCompleted || []).slice(0, 4).forEach((item) => {
        ry = pdfWrapText(doc, `• ${item}`, rightX + 4, ry, colW - 8, 3.6) + 1;
      });
      doc.setFont(undefined, 'bold');
      doc.text('Highlights', rightX + 4, y + 32);
      doc.setFont(undefined, 'normal');
      ry = y + 37;
      (workProgress || []).slice(0, 2).forEach((item) => {
        ry = pdfWrapText(doc, `• ${item}`, rightX + 4, ry, colW - 8, 3.6) + 1;
      });

      return y + blockH + 8;
    }

    function buildStatusReportPdf(payload, todayData, weekData) {
      const doc = new jsPDF();
      const report = payload?.report || {};
      const meta = report.report_meta || {};
      const local = payload?.local_data || {};
      const en = (t) => extractEnglishText(t ?? '');
      const userName = currentUser?.display_name || currentUser?.name || currentUser?.email || 'Developer';
      const period = meta.period_label || (local.period_start && local.period_end
        ? `${local.period_start} — ${local.period_end}`
        : (todayData?.date || new Date().toISOString().slice(0, 10)));
      const focusPct = local.total_seconds > 0
        ? Math.round((local.focus_seconds / local.total_seconds) * 100)
        : 0;
      const generatedAt = payload?.generated_at || new Date().toLocaleDateString(undefined, {
        month: 'long', day: 'numeric', year: 'numeric',
      });

      let y = PDF_MARGIN;
      y = pdfDrawTbiTitle(doc, y, userName, generatedAt);

      y = pdfDrawMetaSummaryRow(doc, y, [
        { label: 'Period', value: period },
        { label: 'Focus target', value: en(report.focus_target || meta.focus_target || '—') },
        { label: 'Tracked hours', value: `${local.total_hours ?? 0}h · ${focusPct}% focus` },
      ], en(report.executive_overview || report.work_summary) || '—');

      y = pdfDrawOverallHealth(doc, y, en(report.overall_health) || 'Attention', en(report.health_notes) || '—');

      const tableRows = (report.health_breakdown || []).map((r) => ({
        element: en(r.element),
        status: en(r.status),
        owner_team: en(r.owner_team || 'Self'),
        notes: en(r.notes),
      }));
      y = pdfDrawTbiTable(doc, y, tableRows);

      const weekDays = weekData?.days?.length
        ? weekData.days
        : (local.daily_totals || []).map((d) => ({ weekday: d.date?.slice(5), total_seconds: d.total_seconds }));
      const categories = (local.category_breakdown || []).slice(0, 6).map((c) => ({
        label: c.category,
        value: c.total_seconds,
        display: `${(c.total_seconds / 3600).toFixed(1)}h`,
      }));
      y = pdfDrawTimelineReview(
        doc,
        y,
        weekDays,
        categories,
        en(report.timeline_caption || report.work_summary) || '—',
      );

      y = pdfDrawRisksProgressColumns(
        doc,
        y,
        (report.known_issues || []).map(en),
        (report.potential_risks || []).map(en),
        (report.tasks_completed || []).map(en),
        (report.work_progress || []).map(en),
      );

      y = pdfSectionLabel(doc, 'Lessons learned', y);
      for (const lesson of (report.lessons_learned || []).slice(0, 3)) {
        y = pdfEnsureSpace(doc, y, 18);
        doc.setFillColor(237, 233, 254);
        doc.roundedRect(PDF_MARGIN, y, PDF_CONTENT_W, 16, 2, 2, 'F');
        doc.setFont(undefined, 'bold');
        doc.setFontSize(8.5);
        doc.setTextColor(80, 60, 140);
        doc.text(en(lesson.title) || 'Lesson', PDF_MARGIN + 4, y + 5);
        doc.setFont(undefined, 'normal');
        doc.setFontSize(7.5);
        doc.setTextColor(70, 70, 80);
        y = pdfWrapText(doc, en(lesson.body), PDF_MARGIN + 4, y + 10, PDF_CONTENT_W - 8, 3.8) + 8;
      }

      if ((report.recommendations || []).length) {
        y = pdfSectionLabel(doc, 'Recommendations', y);
        y = pdfBulletList(doc, report.recommendations.map(en), y);
      }

      y = pdfEnsureSpace(doc, y, 8);
      doc.setFontSize(7);
      doc.setTextColor(150, 150, 160);
      const sectionCount = (payload.generation_passes || []).length;
      doc.text(
        `Generated locally · ${payload?.model || 'Flowmates'} · ${sectionCount} AI sections`,
        PDF_MARGIN,
        y,
      );

      const filename = buildStatusReportPdfFilename(payload, todayData);
      return { doc, filename };
    }

    function pdfBulletList(doc, items, y) {
      doc.setFontSize(9.5);
      doc.setTextColor(60, 60, 60);
      for (const item of items || []) {
        y = pdfEnsureSpace(doc, y, 8);
        const bullet = `\u2022 ${item}`;
        y = pdfWrapText(doc, bullet, PDF_MARGIN + 2, y, PDF_CONTENT_W - 4, 4.5) + 2;
      }
      return y + 2;
    }

    function pdfDrawBarChart(doc, y, title, items, maxVal) {
      y = pdfEnsureSpace(doc, y, 20);
      doc.setFont(undefined, 'bold');
      doc.setFontSize(9);
      doc.setTextColor(80, 80, 90);
      doc.text(title, PDF_MARGIN, y);
      y += 6;
      const barMaxW = PDF_CONTENT_W - 52;
      const colors = [[99, 102, 241], [34, 197, 94], [251, 146, 60], [236, 72, 153], [14, 165, 233], [168, 85, 247]];
      items.forEach((item, idx) => {
        y = pdfEnsureSpace(doc, y, 10);
        const val = item.value || 0;
        const pct = maxVal > 0 ? val / maxVal : 0;
        const barW = Math.max(pct * barMaxW, val > 0 ? 4 : 0);
        const [cr, cg, cb] = colors[idx % colors.length];
        doc.setFontSize(8);
        doc.setTextColor(50, 50, 55);
        doc.text(String(item.label).slice(0, 18), PDF_MARGIN, y + 3);
        doc.setFillColor(235, 235, 240);
        doc.roundedRect(PDF_MARGIN + 42, y - 2, barMaxW, 5, 1, 1, 'F');
        if (barW > 0) {
          doc.setFillColor(cr, cg, cb);
          doc.roundedRect(PDF_MARGIN + 42, y - 2, barW, 5, 1, 1, 'F');
        }
        doc.setTextColor(90, 90, 100);
        doc.text(item.display || '', PDF_MARGIN + 42 + barMaxW + 2, y + 3);
        y += 7;
      });
      return y + 4;
    }

    function pdfDrawWeekChart(doc, y, days) {
      if (!days?.length) return y;
      y = pdfEnsureSpace(doc, y, 38);
      doc.setFont(undefined, 'bold');
      doc.setFontSize(9);
      doc.setTextColor(80, 80, 90);
      doc.text('Weekly activity', PDF_MARGIN, y);
      y += 5;
      const chartH = 24;
      const barW = (PDF_CONTENT_W - (days.length - 1) * 3) / days.length;
      const maxSec = Math.max(...days.map((d) => d.total_seconds || d.seconds || 0), 1);
      days.forEach((d, i) => {
        const sec = d.total_seconds ?? d.seconds ?? 0;
        const h = Math.max(2, (sec / maxSec) * chartH);
        const x = PDF_MARGIN + i * (barW + 3);
        doc.setFillColor(235, 235, 240);
        doc.roundedRect(x, y + chartH - 2, barW, 2, 1, 1, 'F');
        doc.setFillColor(99, 102, 241);
        doc.roundedRect(x, y + chartH - h, barW, h, 1, 1, 'F');
        doc.setFontSize(7);
        doc.setTextColor(100, 100, 110);
        const lbl = d.weekday || (d.date ? d.date.slice(5) : '');
        doc.text(lbl, x + barW / 2, y + chartH + 5, { align: 'center' });
      });
      return y + chartH + 10;
    }

