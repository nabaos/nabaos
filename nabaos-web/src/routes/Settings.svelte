<script lang="ts">
  import { onMount } from 'svelte';
  import {
    getPersonas, setActivePersona, type PersonaList,
    getVault, storeVaultKey, type ProviderInfo,
    getRules, type Rules,
    getToolServers, getTools, type ToolServer, type Tool,
    getAbilities, type Ability,
    securityScan, type ScanResult,
    getEnvKeys, setEnvKey, type EnvKeyInfo,
  } from '../lib/api';
  import { Card, Badge, Modal, Skeleton, StatCard, Button } from '../lib/components';
  import { showToast } from '../lib/stores.svelte';

  // ── Section collapse state ───────────────────────────────────────────
  let collapsed = $state<Record<string, boolean>>({
    personas: false,
    appearance: false,
    vault: false,
    apikeys: false,
    rules: false,
    tools: false,
    security: true,   // starts collapsed
    system: false,
  });

  function toggleSection(key: string) {
    collapsed = { ...collapsed, [key]: !collapsed[key] };
  }

  // ── Shared state ─────────────────────────────────────────────────────
  let error = $state('');
  let loading = $state(true);

  // ── Personas ─────────────────────────────────────────────────────────
  let personas = $state<string[]>([]);
  let activePersona = $state('');
  let switchingPersona = $state('');

  async function selectPersona(id: string) {
    if (id === activePersona || switchingPersona) return;
    switchingPersona = id;
    try {
      const result = await setActivePersona(id);
      activePersona = result.active;
      showToast(`Switched to persona: ${result.active}`, 'success');
    } catch (e: any) {
      showToast('Failed to switch persona', 'error');
    }
    switchingPersona = '';
  }

  // ── Appearance ───────────────────────────────────────────────────────
  let currentTheme = $state(localStorage.getItem('nabaos-theme') || 'system');

  function setTheme(t: string) {
    currentTheme = t;
    localStorage.setItem('nabaos-theme', t);
    const resolved = t === 'system'
      ? (window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark')
      : t;
    document.documentElement.setAttribute('data-theme', resolved);
  }

  // ── Vault ────────────────────────────────────────────────────────────
  let providers = $state<ProviderInfo[]>([]);
  let modalOpen = $state(false);
  let modalProvider = $state<ProviderInfo | null>(null);
  let apiKey = $state('');
  let saving = $state(false);
  let saveError = $state('');

  function openConfigure(provider: ProviderInfo) {
    modalProvider = provider;
    apiKey = '';
    saveError = '';
    modalOpen = true;
  }

  function closeModal() {
    modalOpen = false;
    modalProvider = null;
    apiKey = '';
    saveError = '';
  }

  async function handleSaveKey() {
    if (!modalProvider || !apiKey.trim()) return;
    saving = true;
    saveError = '';
    try {
      const result = await storeVaultKey(modalProvider.id, apiKey.trim());
      if (result.stored) {
        showToast(`API key saved for ${modalProvider.display_name}`, 'success');
        closeModal();
        // reload providers
        try {
          const data = await getVault();
          providers = data.providers;
        } catch {}
      } else {
        saveError = 'Key was not stored. Please check the format and try again.';
      }
    } catch (e: any) {
      saveError = e.message;
    }
    saving = false;
  }

  // ── Rules ────────────────────────────────────────────────────────────
  let rules = $state<Rules | null>(null);
  let openRules = $state(new Set<number>());

  function toggleRule(i: number) {
    const next = new Set(openRules);
    if (next.has(i)) next.delete(i); else next.add(i);
    openRules = next;
  }

  function enforcementVariant(e: string): 'danger' | 'warning' | 'info' {
    switch (e.toLowerCase()) {
      case 'strict': return 'danger';
      case 'warn': return 'warning';
      default: return 'info';
    }
  }

  // ── Tools ────────────────────────────────────────────────────────────
  let toolServers = $state<ToolServer[]>([]);
  let toolsMap = $state<Record<string, Tool[]>>({});
  let expandedServer = $state<string | null>(null);

  // ── Security Scanner ─────────────────────────────────────────────────
  let scanText = $state('');
  let scanResult = $state<ScanResult | null>(null);
  let scanError = $state('');
  let scanLoading = $state(false);
  let showRedacted = $state(false);

  // ── API Keys ──────────────────────────────────────────────────────────
  let envKeys = $state<EnvKeyInfo[]>([]);
  let editingKey = $state('');
  let editingValue = $state('');
  let savingKey = $state(false);

  async function saveEnvKey(name: string) {
    if (!editingValue.trim()) return;
    savingKey = true;
    try {
      await setEnvKey(name, editingValue.trim());
      const data = await getEnvKeys();
      envKeys = data.keys;
      editingKey = '';
      editingValue = '';
      showToast(`${name} updated`, 'success');
    } catch (e: any) {
      showToast(`Failed to update ${name}`, 'error');
    }
    savingKey = false;
  }

  async function handleScan() {
    if (!scanText.trim()) return;
    scanError = '';
    scanResult = null;
    showRedacted = false;
    scanLoading = true;
    try {
      scanResult = await securityScan(scanText);
    } catch (e: any) {
      scanError = e.message;
    }
    scanLoading = false;
  }

  // ── Load all data on mount ───────────────────────────────────────────
  onMount(async () => {
    const errors: string[] = [];

    // Load each section independently so one failure doesn't block the rest
    try {
      const personaData = await getPersonas();
      personas = personaData.personas || [];
      activePersona = personaData.active || '';
    } catch (e: any) { errors.push(`Personas: ${e.message}`); }

    try {
      const vaultData = await getVault();
      providers = vaultData.providers || [];
    } catch (e: any) { errors.push(`Vault: ${e.message}`); }

    try {
      const rulesData = await getRules();
      rules = rulesData;
    } catch (e: any) { errors.push(`Rules: ${e.message}`); }

    try {
      const toolData = await getToolServers();
      toolServers = (toolData && toolData.servers) ? toolData.servers : [];
    } catch { toolServers = []; }

    try {
      const envData = await getEnvKeys();
      envKeys = envData.keys;
    } catch {}

    if (errors.length > 0) {
      error = errors.join('; ');
    }
    loading = false;
  });
</script>

<div class="settings-page">
  <div class="page-header">
    <h1>Settings</h1>
    <p class="subtitle">Configure your agent</p>
  </div>

  {#if error}
    <p class="error-text">{error}</p>
  {/if}

  {#if loading}
    <div class="section">
      <Card title="Personas"><Skeleton width="100%" height="56px" /></Card>
    </div>
    <div class="section">
      <Card title="Appearance"><Skeleton width="60%" height="40px" /></Card>
    </div>
    <div class="section">
      <Card title="Vault — API Keys"><Skeleton width="100%" height="56px" /></Card>
    </div>
    <div class="section">
      <Card title="Rules"><Skeleton width="100%" height="20px" /><div style="margin-top:0.5rem;"><Skeleton width="100%" height="20px" /></div></Card>
    </div>
    <div class="section">
      <Card title="Tools"><Skeleton width="100%" height="20px" /><div style="margin-top:0.5rem;"><Skeleton width="100%" height="20px" /></div></Card>
    </div>
  {:else}

    <!-- 1. Personas -->
    <div class="section">
      <button class="section-toggle" onclick={() => toggleSection('personas')}>
        <span class="section-chevron" class:open={!collapsed.personas}>&#9658;</span>
        <span class="section-title">Personas ({personas.length})</span>
      </button>
      {#if !collapsed.personas}
        <Card>
          {#each personas as persona}
            <button
              class="persona-row"
              class:active={persona === activePersona}
              class:switching={persona === switchingPersona}
              onclick={() => selectPersona(persona)}
              disabled={persona === switchingPersona}
            >
              <div class="persona-icon">
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/>
                  <circle cx="12" cy="7" r="4"/>
                </svg>
              </div>
              <span class="persona-name">{persona}</span>
              <div class="persona-status">
                {#if persona === switchingPersona}
                  <Badge variant="warning">switching...</Badge>
                {:else if persona === activePersona}
                  <Badge variant="success">active</Badge>
                {:else}
                  <span class="activate-hint">Click to activate</span>
                {/if}
              </div>
            </button>
          {:else}
            <p style="color: var(--text-dim); font-size: 0.9rem;">No personas registered. Add persona TOML files to the config/personas/ directory.</p>
          {/each}
        </Card>
      {/if}
    </div>

    <!-- 2. Appearance -->
    <div class="section">
      <button class="section-toggle" onclick={() => toggleSection('appearance')}>
        <span class="section-chevron" class:open={!collapsed.appearance}>&#9658;</span>
        <span class="section-title">Appearance</span>
      </button>
      {#if !collapsed.appearance}
        <Card>
          <div class="theme-options">
            {#each ['dark', 'light', 'system'] as t}
              <label class="theme-option" class:selected={currentTheme === t}>
                <input type="radio" name="theme" value={t} bind:group={currentTheme} onchange={() => setTheme(t)} />
                <span class="theme-icon">
                  {#if t === 'dark'}🌙{:else if t === 'light'}☀️{:else}🖥️{/if}
                </span>
                <span class="theme-name">{t}</span>
              </label>
            {/each}
          </div>
        </Card>
      {/if}
    </div>

    <!-- 3. Vault — API Keys -->
    <div class="section">
      <button class="section-toggle" onclick={() => toggleSection('vault')}>
        <span class="section-chevron" class:open={!collapsed.vault}>&#9658;</span>
        <span class="section-title">Vault — API Keys ({providers.length})</span>
      </button>
      {#if !collapsed.vault}
        <Card>
          {#each providers as provider}
            <div class="provider-row">
              <div class="provider-icon">
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <rect x="2" y="2" width="20" height="8" rx="2" ry="2"/>
                  <rect x="2" y="14" width="20" height="8" rx="2" ry="2"/>
                  <line x1="6" y1="6" x2="6.01" y2="6"/>
                  <line x1="6" y1="18" x2="6.01" y2="18"/>
                </svg>
              </div>
              <div class="provider-info">
                <span class="provider-name">{provider.display_name}</span>
                <span class="provider-id">{provider.id}</span>
              </div>
              <div class="provider-status">
                {#if provider.configured}
                  <Badge variant="success">configured</Badge>
                {:else}
                  <Badge variant="neutral">not configured</Badge>
                {/if}
              </div>
              <button class="btn btn-sm" class:btn-primary={!provider.configured} onclick={() => openConfigure(provider)}>
                {provider.configured ? 'Update Key' : 'Configure'}
              </button>
            </div>
          {:else}
            <p style="color: var(--text-dim); font-size: 0.9rem;">No providers available.</p>
          {/each}
          <p class="vault-note">
            API keys are encrypted and stored locally. They are never sent to any third party.
          </p>
        </Card>
      {/if}
    </div>

    <!-- 4. API Keys -->
    <div class="section">
      <button class="section-toggle" onclick={() => toggleSection('apikeys')}>
        <span class="section-chevron" class:open={!collapsed.apikeys}>&#9658;</span>
        <span class="section-title">API Keys ({envKeys.length})</span>
      </button>
      {#if !collapsed.apikeys}
        <Card>
          <p class="vault-note" style="margin-top: 0; padding-top: 0; border-top: none;">Manage API keys for image sourcing and integrations. Values are never displayed.</p>
          <div class="env-keys-list">
            {#each envKeys as key}
              <div class="env-key-row">
                <div class="env-key-info">
                  <code class="env-key-name">{key.name}</code>
                  <span class="env-key-desc">{key.description}</span>
                </div>
                <div class="env-key-actions">
                  {#if key.is_set}
                    <Badge variant="success">SET</Badge>
                  {:else}
                    <Badge variant="neutral">NOT SET</Badge>
                  {/if}
                  {#if editingKey === key.name}
                    <input
                      type="password"
                      class="env-key-input"
                      placeholder="Enter value..."
                      bind:value={editingValue}
                      onkeydown={(e) => { if (e.key === 'Enter') saveEnvKey(key.name); }}
                    />
                    <Button size="sm" onclick={() => saveEnvKey(key.name)} disabled={savingKey}>
                      {savingKey ? 'Saving...' : 'Save'}
                    </Button>
                    <Button size="sm" variant="ghost" onclick={() => { editingKey = ''; editingValue = ''; }}>
                      Cancel
                    </Button>
                  {:else}
                    <Button size="sm" variant="ghost" onclick={() => { editingKey = key.name; editingValue = ''; }}>
                      Edit
                    </Button>
                  {/if}
                </div>
              </div>
            {:else}
              <p style="color: var(--text-dim); font-size: 0.9rem;">No managed env keys available.</p>
            {/each}
          </div>
        </Card>
      {/if}
    </div>

    <!-- 5. Rules -->
    <div class="section">
      <button class="section-toggle" onclick={() => toggleSection('rules')}>
        <span class="section-chevron" class:open={!collapsed.rules}>&#9658;</span>
        <span class="section-title">Rules{rules ? `: ${rules.name}` : ''}</span>
      </button>
      {#if !collapsed.rules && rules}
        <Card>
          {#each rules.rules as rule, i}
            <div class="rule-item">
              <button class="rule-header" onclick={() => toggleRule(i)}>
                <span class="rule-name">{rule.name}</span>
                <Badge variant={enforcementVariant(rule.enforcement)}>{rule.enforcement}</Badge>
                <span class="rule-chevron" class:open={openRules.has(i)}>&#9658;</span>
              </button>
              {#if openRules.has(i)}
                <div class="rule-body fade-in">
                  {#if rule.description}<p class="rule-desc">{rule.description}</p>{/if}
                  {#if rule.trigger_actions.length > 0}
                    <div class="rule-tags"><strong>Actions:</strong> {#each rule.trigger_actions as a}<Badge variant="info">{a}</Badge>{/each}</div>
                  {/if}
                  {#if rule.trigger_targets.length > 0}
                    <div class="rule-tags"><strong>Targets:</strong> {#each rule.trigger_targets as t}<Badge variant="info">{t}</Badge>{/each}</div>
                  {/if}
                  {#if rule.trigger_keywords.length > 0}
                    <div class="rule-tags"><strong>Keywords:</strong> {#each rule.trigger_keywords as k}<Badge variant="neutral">{k}</Badge>{/each}</div>
                  {/if}
                </div>
              {/if}
            </div>
          {/each}
        </Card>
      {/if}
    </div>

    <!-- 5. Tools -->
    <div class="section">
      <button class="section-toggle" onclick={() => toggleSection('tools')}>
        <span class="section-chevron" class:open={!collapsed.tools}>&#9658;</span>
        <span class="section-title">Tools ({toolServers.length})</span>
      </button>
      {#if !collapsed.tools}
        <Card>
          {#each toolServers as server}
            <div class="tool-row">
              <button class="tool-header" onclick={async () => {
                if (expandedServer === server.id) {
                  expandedServer = null;
                } else {
                  expandedServer = server.id;
                  if (!toolsMap[server.id]) {
                    try {
                      const data = await getTools(server.id);
                      toolsMap = { ...toolsMap, [server.id]: data.tools || [] };
                    } catch {
                      toolsMap = { ...toolsMap, [server.id]: [] };
                    }
                  }
                }
              }}>
                <span class="tool-status" class:running={server.status === 'running'}></span>
                <span class="tool-server-name">{server.id}</span>
                <Badge variant="neutral">{server.trust_level}</Badge>
                <span class="tool-count">{server.tool_count} tools</span>
                <span class="rule-chevron" class:open={expandedServer === server.id}>&#9658;</span>
              </button>
              {#if expandedServer === server.id && toolsMap[server.id]}
                <div class="tool-list fade-in">
                  {#each toolsMap[server.id] as tool}
                    <div class="tool-item">
                      <span class="tool-name">{tool.name}</span>
                      <span class="tool-desc">{tool.description}</span>
                    </div>
                  {/each}
                  {#if toolsMap[server.id].length === 0}
                    <p style="color: var(--text-dim); font-size: 0.85rem;">No tools discovered yet.</p>
                  {/if}
                </div>
              {/if}
            </div>
          {:else}
            <p style="color: var(--text-dim); font-size: 0.9rem;">No tool servers configured for this agent.</p>
          {/each}
        </Card>
      {/if}
    </div>

    <!-- 6. Security Scanner -->
    <div class="section">
      <button class="section-toggle" onclick={() => toggleSection('security')}>
        <span class="section-chevron" class:open={!collapsed.security}>&#9658;</span>
        <span class="section-title">Security Scanner</span>
      </button>
      {#if !collapsed.security}
        <Card>
          <div class="form-group" style="margin-bottom: 0.5rem;">
            <label>Text to Scan</label>
            <textarea bind:value={scanText} placeholder="Paste text to scan..."></textarea>
          </div>
          <div class="char-count" class:warn={scanText.length > 3500} class:danger={scanText.length > 4000}>
            {scanText.length} / 4,096
          </div>
          <Button variant="primary" loading={scanLoading} onclick={handleScan}>Scan</Button>

          {#if scanError}
            <p class="error-text">{scanError}</p>
          {/if}

          {#if scanResult}
            <div class="slide-up" style="margin-top: 1.5rem;">
              {#if scanResult.credential_count === 0 && scanResult.pii_count === 0 && !scanResult.injection_detected}
                <div class="clean-result fade-in" style="text-align: center; padding: 2rem;">
                  <svg class="check-icon" width="64" height="64" viewBox="0 0 24 24" fill="none" stroke="var(--success)" stroke-width="2">
                    <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/>
                    <polyline points="22 4 12 14.01 9 11.01"/>
                  </svg>
                  <h3 style="color: var(--success); margin-top: 1rem;">All clear</h3>
                  <p style="color: var(--text-dim);">No credentials, PII, or injection patterns detected.</p>
                </div>
              {:else}
                <div class="stats-grid" style="margin-bottom: 1.25rem;">
                  <StatCard value={scanResult.credential_count} label="Credentials" color={scanResult.credential_count > 0 ? 'danger' : 'default'} />
                  <StatCard value={scanResult.pii_count} label="PII Found" color={scanResult.pii_count > 0 ? 'warning' : 'default'} />
                  <StatCard value={scanResult.injection_match_count} label="Injection Matches" color={scanResult.injection_match_count > 0 ? 'danger' : 'default'} />
                </div>

                {#if scanResult.injection_detected}
                  <div class="scan-detail-card">
                    <h4>Injection Analysis</h4>
                    <div style="display: flex; align-items: center; gap: 1rem; margin-bottom: 0.75rem;">
                      <Badge variant="danger">Detected</Badge>
                      <Badge variant="info">{scanResult.injection_category || 'Unknown'}</Badge>
                    </div>
                    <div class="confidence-bar">
                      <div class="confidence-fill" style="width: {(scanResult.injection_confidence * 100)}%; background: {scanResult.injection_confidence > 0.7 ? 'var(--danger)' : scanResult.injection_confidence > 0.4 ? 'var(--warning)' : 'var(--success)'};">
                      </div>
                    </div>
                    <div style="font-size: 0.78rem; color: var(--text-dim); margin-top: 0.25rem;">
                      Confidence: {(scanResult.injection_confidence * 100).toFixed(0)}%
                    </div>
                  </div>
                {/if}

                {#if scanResult.credential_types.length > 0}
                  <div class="scan-detail-card">
                    <h4>Credential Types</h4>
                    <div style="display: flex; flex-wrap: wrap; gap: 0.5rem;">
                      {#each scanResult.credential_types as ctype}
                        <Badge variant="danger">{ctype}</Badge>
                      {/each}
                    </div>
                  </div>
                {/if}

                {#if scanResult.redacted}
                  <div style="margin-top: 1rem;">
                    <button class="btn btn-ghost btn-sm" onclick={() => showRedacted = !showRedacted}>
                      {showRedacted ? 'Hide' : 'Show'} redacted output
                    </button>
                    {#if showRedacted}
                      <pre class="code-block fade-in" style="margin-top: 0.5rem;">{scanResult.redacted}</pre>
                    {/if}
                  </div>
                {/if}
              {/if}
            </div>
          {/if}
        </Card>
      {/if}
    </div>

    <!-- 7. System -->
    <div class="section">
      <button class="section-toggle" onclick={() => toggleSection('system')}>
        <span class="section-chevron" class:open={!collapsed.system}>&#9658;</span>
        <span class="section-title">System</span>
      </button>
      {#if !collapsed.system}
        <Card>
          <div class="system-info">
            <div class="info-row"><span class="info-label">Version</span><span class="mono">0.2.3</span></div>
            <div class="info-row"><span class="info-label">Runtime</span><span>Rust + Tokio</span></div>
            <div class="info-row"><span class="info-label">Frontend</span><span>Svelte 5 + Vite</span></div>
          </div>
        </Card>
      {/if}
    </div>
  {/if}
</div>

<!-- Vault API Key Modal -->
<Modal open={modalOpen} onclose={closeModal} title={modalProvider ? `Configure ${modalProvider.display_name}` : 'Configure Provider'}>
  <form class="key-form" onsubmit={(e) => { e.preventDefault(); handleSaveKey(); }}>
    <label class="key-label" for="api-key-input">API Key</label>
    <input
      id="api-key-input"
      type="password"
      class="key-input"
      placeholder="Enter API key..."
      bind:value={apiKey}
      autocomplete="off"
    />
    {#if saveError}
      <p class="error-text" style="margin-top: 0.5rem;">{saveError}</p>
    {/if}
    <div class="key-actions">
      <button type="button" class="btn btn-sm" onclick={closeModal}>Cancel</button>
      <button type="submit" class="btn btn-sm btn-primary" disabled={saving || !apiKey.trim()}>
        {saving ? 'Saving...' : 'Save Key'}
      </button>
    </div>
  </form>
</Modal>

<style>
  .settings-page { max-width: 800px; margin: 0 auto; }

  .page-header { margin-bottom: 1.5rem; }
  .page-header h1 { margin: 0; }
  .subtitle { color: var(--text-dim); font-size: 0.9rem; margin-top: 0.25rem; }

  .section { margin-bottom: 1rem; }

  /* ── Section toggle (collapsible header) ─────────────────────────── */
  .section-toggle {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    width: 100%;
    padding: 0.65rem 0.25rem;
    background: none;
    border: none;
    border-bottom: 1px solid var(--border-subtle);
    color: var(--text);
    cursor: pointer;
    font-size: 1rem;
    text-align: left;
    transition: color var(--transition-fast);
  }
  .section-toggle:hover { color: var(--accent); }
  .section-chevron {
    color: var(--text-muted);
    transition: transform var(--transition-fast);
    font-size: 0.7rem;
    flex-shrink: 0;
  }
  .section-chevron.open { transform: rotate(90deg); }
  .section-title { font-weight: 700; }

  /* ── Appearance ──────────────────────────────────────────────────── */
  .theme-options { display: flex; gap: 1rem; }
  .theme-option { display: flex; flex-direction: column; align-items: center; gap: 0.5rem; padding: 1rem 1.5rem; border: 1px solid var(--border); border-radius: var(--radius-md); cursor: pointer; transition: all var(--transition-fast); text-transform: capitalize; }
  .theme-option:hover { background: var(--bg-hover); }
  .theme-option.selected { border-color: var(--accent); background: var(--accent-subtle); }
  .theme-option input { display: none; }
  .theme-icon { font-size: 1.5rem; }
  .theme-name { font-size: 0.82rem; color: var(--text-dim); }

  /* ── Personas ────────────────────────────────────────────────────── */
  .persona-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    width: 100%;
    padding: 0.75rem 0.5rem;
    background: none;
    border: none;
    border-bottom: 1px solid var(--border-subtle);
    color: var(--text);
    cursor: pointer;
    font-size: 0.9rem;
    text-align: left;
    border-radius: 0;
    transition: background var(--transition-fast);
  }
  .persona-row:last-child { border-bottom: none; }
  .persona-row:hover { background: var(--bg-hover); }
  .persona-row.active { background: var(--accent-subtle); }
  .persona-row.switching { opacity: 0.7; cursor: wait; }

  .persona-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    background: var(--bg-hover);
    flex-shrink: 0;
  }
  .persona-row.active .persona-icon {
    background: var(--accent);
    color: var(--bg);
  }

  .persona-name {
    flex: 1;
    font-weight: 600;
    font-family: var(--font-mono, 'JetBrains Mono', monospace);
    font-size: 0.88rem;
  }

  .persona-status { flex-shrink: 0; }

  .activate-hint {
    color: var(--text-muted);
    font-size: 0.8rem;
    opacity: 0;
    transition: opacity var(--transition-fast);
  }
  .persona-row:hover .activate-hint { opacity: 1; }

  /* ── Vault (Providers) ──────────────────────────────────────────── */
  .provider-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.75rem 0.25rem;
    border-bottom: 1px solid var(--border-subtle);
  }
  .provider-row:last-child { border-bottom: none; }

  .provider-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    border-radius: var(--radius-md);
    background: var(--bg-hover);
    flex-shrink: 0;
    color: var(--text-dim);
  }

  .provider-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    min-width: 0;
  }
  .provider-name { font-weight: 600; font-size: 0.9rem; }
  .provider-id { font-family: var(--font-mono, 'JetBrains Mono', monospace); font-size: 0.78rem; color: var(--text-muted); }
  .provider-status { flex-shrink: 0; }

  .vault-note {
    color: var(--text-dim);
    font-size: 0.82rem;
    line-height: 1.5;
    margin: 0.75rem 0 0;
    padding-top: 0.75rem;
    border-top: 1px solid var(--border-subtle);
  }

  /* Modal form */
  .key-form { display: flex; flex-direction: column; gap: 0.5rem; }
  .key-label { font-size: 0.85rem; font-weight: 600; color: var(--text-dim); }
  .key-input {
    width: 100%;
    padding: 0.6rem 0.75rem;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    color: var(--text);
    font-size: 0.9rem;
    font-family: var(--font-mono, 'JetBrains Mono', monospace);
    box-sizing: border-box;
  }
  .key-input:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-subtle);
  }
  .key-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.75rem;
  }

  /* ── Rules ──────────────────────────────────────────────────────── */
  .rule-item { border-bottom: 1px solid var(--border-subtle); }
  .rule-item:last-child { border-bottom: none; }
  .rule-header { display: flex; align-items: center; gap: 0.75rem; width: 100%; padding: 0.75rem 0; background: none; border: none; color: var(--text); cursor: pointer; font-size: 0.9rem; text-align: left; }
  .rule-header:hover { color: var(--accent); }
  .rule-name { flex: 1; font-weight: 600; }
  .rule-chevron { color: var(--text-muted); transition: transform var(--transition-fast); font-size: 0.75rem; }
  .rule-chevron.open { transform: rotate(90deg); }
  .rule-body { padding: 0 0 0.75rem 0; }
  .rule-desc { color: var(--text-dim); font-size: 0.85rem; margin-bottom: 0.5rem; line-height: 1.5; }
  .rule-tags { display: flex; flex-wrap: wrap; gap: 0.35rem; align-items: center; margin-bottom: 0.35rem; font-size: 0.82rem; }
  .rule-tags strong { margin-right: 0.25rem; color: var(--text-dim); }

  /* ── Tools ──────────────────────────────────────────────────────── */
  .tool-row { border-bottom: 1px solid var(--border-subtle); }
  .tool-row:last-child { border-bottom: none; }
  .tool-header { display: flex; align-items: center; gap: 0.75rem; width: 100%; padding: 0.75rem 0; background: none; border: none; color: var(--text); cursor: pointer; font-size: 0.9rem; text-align: left; }
  .tool-header:hover { color: var(--accent); }
  .tool-server-name { font-weight: 600; flex: 1; }
  .tool-status { width: 8px; height: 8px; border-radius: 50%; background: var(--text-muted); flex-shrink: 0; }
  .tool-status.running { background: var(--success); }
  .tool-count { color: var(--text-dim); font-size: 0.82rem; }
  .tool-list { padding: 0 0 0.75rem 1.5rem; }
  .tool-item { display: flex; gap: 0.75rem; padding: 0.3rem 0; font-size: 0.85rem; }
  .tool-name { font-family: var(--font-mono, 'JetBrains Mono', monospace); font-weight: 600; min-width: 150px; }
  .tool-desc { color: var(--text-dim); }

  /* ── Security Scanner ───────────────────────────────────────────── */
  .char-count { font-size: 0.75rem; color: var(--text-muted); text-align: right; margin-bottom: 1rem; }
  .char-count.warn { color: var(--warning); }
  .char-count.danger { color: var(--danger); }
  .confidence-bar { height: 8px; background: var(--bg-hover); border-radius: 4px; overflow: hidden; }
  .confidence-fill { height: 100%; border-radius: 4px; transition: width 0.5s ease; }
  .check-icon { animation: checkPop 0.4s ease-out; }
  @keyframes checkPop {
    0% { transform: scale(0); opacity: 0; }
    60% { transform: scale(1.15); }
    100% { transform: scale(1); opacity: 1; }
  }
  .scan-detail-card {
    margin-top: 1rem;
    padding: 0.75rem;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
  }
  .scan-detail-card h4 {
    margin: 0 0 0.5rem;
    font-size: 0.9rem;
    font-weight: 600;
  }

  /* ── System ─────────────────────────────────────────────────────── */
  .system-info { display: flex; flex-direction: column; gap: 0.5rem; }
  .info-row { display: flex; justify-content: space-between; padding: 0.4rem 0; border-bottom: 1px solid var(--border-subtle); font-size: 0.88rem; }
  .info-row:last-child { border-bottom: none; }
  .info-label { color: var(--text-dim); }

  /* ── API Keys ──────────────────────────────────────────────────── */
  .env-keys-list { display: flex; flex-direction: column; gap: 0.5rem; }
  .env-key-row { display: flex; justify-content: space-between; align-items: center; padding: 0.5rem 0; border-bottom: 1px solid rgba(255,255,255,0.06); flex-wrap: wrap; gap: 0.5rem; }
  .env-key-info { display: flex; flex-direction: column; gap: 0.15rem; }
  .env-key-name { font-size: 0.8rem; color: #ffaf5f; }
  .env-key-desc { font-size: 0.72rem; color: #888; }
  .env-key-actions { display: flex; align-items: center; gap: 0.4rem; }
  .env-key-input { background: #1a1a24; border: 1px solid #333; border-radius: 4px; padding: 0.25rem 0.5rem; color: #c8c8d2; font-size: 0.8rem; width: 180px; }
</style>
