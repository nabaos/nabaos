// ── Types ──────────────────────────────────────────────────────────────

export interface AuthStatus {
  authenticated: boolean;
  auth_required: boolean;
}

export interface DashboardData {
  total_workflows: number;
  total_scheduled_jobs: number;
  total_abilities: number;
  costs: CostData;
}

export interface SecurityInfo {
  credentials_found: number;
  injection_detected: boolean;
  injection_confidence: number;
  was_redacted: boolean;
}

export interface QueryResponse {
  tier: string;
  intent_key: string;
  confidence: number;
  allowed: boolean;
  latency_ms: number;
  description: string;
  response_text: string;
  nyaya_mode: string;
  security: SecurityInfo;
}

export interface Workflow {
  workflow_id: string;
  name: string;
  description: string;
  trust_level: number;
  run_count: number;
  success_count: number;
  created_at: string;
}

// Raw workflow shape from backend (may only have id + name)
interface RawWorkflow {
  id?: string;
  workflow_id?: string;
  name: string;
  description?: string;
  trust_level?: number;
  run_count?: number;
  success_count?: number;
  created_at?: string;
}

export interface ScheduledJob {
  id: string;
  workflow_id: string;
  interval_secs: number;
  enabled: boolean;
  run_count: number;
  created_at: string;
}

export interface CostData {
  total_spent_usd: number;
  total_saved_usd: number;
  savings_percent: number;
  total_llm_calls: number;
  total_cache_hits: number;
  total_input_tokens: number;
  total_output_tokens: number;
}

export interface ScanResult {
  credential_count: number;
  pii_count: number;
  credential_types: string[];
  injection_detected: boolean;
  injection_match_count: number;
  injection_confidence: number;
  injection_category: string;
  redacted: string;
}

export interface Ability {
  name: string;
  source: string;
  description: string;
}

export interface ConstitutionRule {
  name: string;
  enforcement: string;
  description: string;
  trigger_actions: string[];
  trigger_targets: string[];
  trigger_keywords: string[];
}

export interface Rules {
  name: string;
  rules: ConstitutionRule[];
}

// ── Token Management ───────────────────────────────────────────────────

const TOKEN_KEY = 'nabaos_token';

function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}

function setToken(token: string) {
  localStorage.setItem(TOKEN_KEY, token);
}

function clearToken() {
  localStorage.removeItem(TOKEN_KEY);
}

export function isLoggedIn(): boolean {
  return getToken() !== null;
}

// ── Request Helper ─────────────────────────────────────────────────────

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  const token = getToken();
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }
  const res = await fetch(path, {
    method,
    headers,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  if (res.status === 401) {
    clearToken();
    throw new Error('Unauthorized');
  }
  if (!res.ok) {
    const text = await res.text();
    throw new Error(text || `HTTP ${res.status}`);
  }
  if (res.status === 200 && res.headers.get('content-type')?.includes('application/json')) {
    return res.json();
  }
  return {} as T;
}

// ── Auth ───────────────────────────────────────────────────────────────

export async function login(password: string): Promise<boolean> {
  const data = await request<{ token: string }>('POST', '/api/v1/auth/login', { password });
  if (data.token) {
    setToken(data.token);
    return true;
  }
  return false;
}

export async function logout(): Promise<void> {
  try {
    await request<void>('POST', '/api/v1/auth/logout');
  } finally {
    clearToken();
  }
}

export async function checkAuth(): Promise<AuthStatus> {
  return request<AuthStatus>('GET', '/api/v1/auth/status');
}

// ── Dashboard ──────────────────────────────────────────────────────────

export async function getDashboard(): Promise<DashboardData> {
  try {
    const raw = await request<any>('GET', '/api/v1/dashboard');
    // Normalize: the backend may return cost data in varying shapes
    const costs = raw?.costs ?? raw ?? {};
    return {
      total_workflows: raw?.total_workflows ?? 0,
      total_scheduled_jobs: raw?.total_scheduled_jobs ?? 0,
      total_abilities: raw?.total_abilities ?? 0,
      costs: {
        total_spent_usd: costs?.total_spent_usd ?? 0,
        total_saved_usd: costs?.total_saved_usd ?? 0,
        savings_percent: costs?.savings_percent ?? 0,
        total_llm_calls: costs?.total_llm_calls ?? 0,
        total_cache_hits: costs?.total_cache_hits ?? 0,
        total_input_tokens: costs?.total_input_tokens ?? 0,
        total_output_tokens: costs?.total_output_tokens ?? 0,
      },
    };
  } catch {
    // If /api/v1/dashboard doesn't exist, build from /api/v1/status
    try {
      const status = await request<any>('GET', '/api/v1/status');
      return {
        total_workflows: 0,
        total_scheduled_jobs: 0,
        total_abilities: 0,
        costs: {
          total_spent_usd: status?.total_spent_usd ?? 0,
          total_saved_usd: status?.total_saved_usd ?? 0,
          savings_percent: status?.savings_percent ?? 0,
          total_llm_calls: status?.total_llm_calls ?? 0,
          total_cache_hits: status?.total_cache_hits ?? 0,
          total_input_tokens: status?.total_input_tokens ?? 0,
          total_output_tokens: status?.total_output_tokens ?? 0,
        },
      };
    } catch {
      return {
        total_workflows: 0, total_scheduled_jobs: 0, total_abilities: 0,
        costs: { total_spent_usd: 0, total_saved_usd: 0, savings_percent: 0, total_llm_calls: 0, total_cache_hits: 0, total_input_tokens: 0, total_output_tokens: 0 },
      };
    }
  }
}

// ── Query ──────────────────────────────────────────────────────────────

export async function sendQuery(query: string): Promise<QueryResponse> {
  return request<QueryResponse>('POST', '/api/v1/ask', { query });
}

// ── Streaming Query (SSE) ──────────────────────────────────────────────

export interface StreamCallbacks {
  onDelta: (text: string) => void;
  onTier?: (info: { tier: string; confidence: number }) => void;
  onDone?: (meta: QueryResponse) => void;
  onError?: (error: string) => void;
}

export async function sendQueryStream(query: string, callbacks: StreamCallbacks): Promise<void> {
  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  const token = getToken();
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }

  const res = await fetch('/api/v1/ask/stream', {
    method: 'POST',
    headers,
    body: JSON.stringify({ query }),
  });

  if (!res.ok) {
    if (res.status === 401) {
      clearToken();
      throw new Error('Unauthorized');
    }
    const text = await res.text();
    throw new Error(text || `HTTP ${res.status}`);
  }

  const reader = res.body?.getReader();
  if (!reader) throw new Error('No response body');

  const decoder = new TextDecoder();
  let buffer = '';

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;

    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop() || '';

    let currentEvent = '';
    for (const line of lines) {
      if (line.startsWith('event:')) {
        currentEvent = line.slice(6).trim();
      } else if (line.startsWith('data:')) {
        const data = line.slice(5).trim();
        switch (currentEvent) {
          case 'tier':
            if (callbacks.onTier) {
              try { callbacks.onTier(JSON.parse(data)); } catch { /* ignore */ }
            }
            break;
          case 'delta':
            callbacks.onDelta(data);
            break;
          case 'done':
            if (callbacks.onDone) {
              try { callbacks.onDone(JSON.parse(data)); } catch { /* ignore */ }
            }
            break;
          case 'error':
            if (callbacks.onError) callbacks.onError(data);
            break;
        }
        currentEvent = '';
      }
    }
  }
}

// ── Workflows ──────────────────────────────────────────────────────────

export async function getWorkflows(): Promise<Workflow[]> {
  try {
    const raw = await request<any>('GET', '/api/v1/workflows');
    // Backend may return { workflows: [...] } or a flat array
    const list: RawWorkflow[] = Array.isArray(raw) ? raw : (raw?.workflows ?? []);
    return list.map((w) => ({
      workflow_id: w.workflow_id || w.id || '',
      name: w.name || 'Unnamed',
      description: w.description || '',
      trust_level: w.trust_level ?? 0,
      run_count: w.run_count ?? 0,
      success_count: w.success_count ?? 0,
      created_at: w.created_at || '',
    }));
  } catch {
    return [];
  }
}

export async function getScheduledJobs(): Promise<ScheduledJob[]> {
  try {
    const raw = await request<any>('GET', '/api/v1/workflows/schedule');
    return Array.isArray(raw) ? raw : (raw?.jobs ?? raw?.schedule ?? []);
  } catch {
    return [];
  }
}

export async function scheduleWorkflow(workflow_id: string, interval: string): Promise<{ job_id: string }> {
  return request<{ job_id: string }>('POST', '/api/v1/workflows/schedule', { workflow_id, interval });
}

export async function disableJob(id: string): Promise<void> {
  return request<void>('DELETE', `/api/v1/workflows/schedule/${id}`);
}

// ── Status ─────────────────────────────────────────────────────────────

export async function getCosts(sinceMs?: number): Promise<CostData> {
  try {
    const path = sinceMs !== undefined ? `/api/v1/status?since=${sinceMs}` : '/api/v1/status';
    const raw = await request<any>('GET', path);
    return {
      total_spent_usd: raw?.total_spent_usd ?? 0,
      total_saved_usd: raw?.total_saved_usd ?? 0,
      savings_percent: raw?.savings_percent ?? 0,
      total_llm_calls: raw?.total_llm_calls ?? 0,
      total_cache_hits: raw?.total_cache_hits ?? 0,
      total_input_tokens: raw?.total_input_tokens ?? 0,
      total_output_tokens: raw?.total_output_tokens ?? 0,
    };
  } catch {
    return { total_spent_usd: 0, total_saved_usd: 0, savings_percent: 0, total_llm_calls: 0, total_cache_hits: 0, total_input_tokens: 0, total_output_tokens: 0 };
  }
}

// ── Security ───────────────────────────────────────────────────────────

export async function securityScan(text: string): Promise<ScanResult> {
  return request<ScanResult>('POST', '/api/v1/security/scan', { text });
}

// ── Abilities ──────────────────────────────────────────────────────────

export async function getAbilities(): Promise<Ability[]> {
  try {
    const raw = await request<any>('GET', '/api/v1/status/abilities');
    return Array.isArray(raw) ? raw : (raw?.abilities ?? []);
  } catch {
    return [];
  }
}

// ── Rules ──────────────────────────────────────────────────────────────

export async function getRules(): Promise<Rules> {
  try {
    const raw = await request<any>('GET', '/api/v1/rules');
    return { name: raw?.name || '', rules: Array.isArray(raw?.rules) ? raw.rules : [] };
  } catch {
    return { name: '', rules: [] };
  }
}

// ── Personas ──────────────────────────────────────────────────────────

export interface PersonaList {
  personas: string[];
  active: string;
}

export async function getPersonas(): Promise<PersonaList> {
  try {
    const raw = await request<any>('GET', '/api/v1/personas');
    // Backend returns { agents: [...], active: "..." } not { personas: [...] }
    const list = raw?.personas || raw?.agents || [];
    return { personas: list, active: raw?.active || '' };
  } catch {
    return { personas: [], active: '' };
  }
}

export async function setActivePersona(persona_id: string): Promise<{ active: string }> {
  return request<{ active: string }>('POST', '/api/v1/personas/active', { persona_id });
}

// ── Vault ──────────────────────────────────────────────────────────────

export interface ProviderInfo {
  id: string;
  display_name: string;
  configured: boolean;
}

export async function getVault(): Promise<{ providers: ProviderInfo[] }> {
  return request<{ providers: ProviderInfo[] }>('GET', '/api/v1/vault');
}

export async function storeVaultKey(provider_id: string, api_key: string): Promise<{ stored: boolean }> {
  return request<{ stored: boolean }>('POST', '/api/v1/vault/store', { provider_id, api_key });
}

// ── Tools ──────────────────────────────────────────────────────────────

export interface ToolServer {
  id: string;
  trust_level: string;
  tool_count: number;
  status: string;
}

export interface Tool {
  name: string;
  description: string;
}

export async function getToolServers(): Promise<{ servers: ToolServer[] }> {
  return request<{ servers: ToolServer[] }>('GET', '/api/v1/tools/servers');
}

export async function getTools(serverId: string): Promise<{ tools: Tool[] }> {
  return request<{ tools: Tool[] }>('GET', `/api/v1/tools/${serverId}`);
}

export async function storeToolSecret(secretName: string, secretValue: string): Promise<{ stored: boolean }> {
  return request<{ stored: boolean }>('POST', '/api/v1/tools/secret', { secret_name: secretName, secret_value: secretValue });
}

export async function discoverTools(serverId: string): Promise<{ discovered: boolean; tools?: Tool[] }> {
  return request<{ discovered: boolean; tools?: Tool[] }>('POST', '/api/v1/tools/discover', { server_id: serverId });
}

// ── System Status ─────────────────────────────────────────────────────
export interface SystemStatus {
  version: string;
  uptime_secs: number;
  channels: string[];
  watcher_enabled: boolean;
  watcher_alerts: number;
  watcher_paused: number;
}

export async function getSystemStatus(): Promise<SystemStatus> {
  try {
    const raw = await request<any>('GET', '/api/v1/status');
    // Backend may return CostData shape instead of SystemStatus — guard all fields
    return {
      version: raw?.version || '',
      uptime_secs: raw?.uptime_secs ?? 0,
      channels: Array.isArray(raw?.channels) ? raw.channels : [],
      watcher_enabled: raw?.watcher_enabled ?? false,
      watcher_alerts: raw?.watcher_alerts ?? 0,
      watcher_paused: raw?.watcher_paused ?? 0,
    };
  } catch {
    return { version: '', uptime_secs: 0, channels: [], watcher_enabled: false, watcher_alerts: 0, watcher_paused: 0 };
  }
}

// ── Costs Dashboard ───────────────────────────────────────────────────
export interface CostPeriod {
  total_cost: number;
  total_calls: number;
  cache_hits: number;
  total_saved: number;
  cache_hit_rate?: number;
}

export interface CostsDashboard {
  daily: CostPeriod;
  weekly: CostPeriod;
  monthly: CostPeriod;
  all_time: CostPeriod;
}

const ZERO_PERIOD: CostPeriod = { total_cost: 0, total_calls: 0, cache_hits: 0, total_saved: 0, cache_hit_rate: 0 };

function normPeriod(p: any): CostPeriod {
  if (!p || typeof p !== 'object') return { ...ZERO_PERIOD };
  return {
    total_cost: p.total_cost ?? p.total_spent ?? p.total_spent_usd ?? 0,
    total_calls: p.total_calls ?? p.total_llm_calls ?? 0,
    cache_hits: p.cache_hits ?? p.total_cache_hits ?? 0,
    total_saved: p.total_saved ?? p.total_saved_usd ?? 0,
    cache_hit_rate: p.cache_hit_rate ?? 0,
  };
}

export async function getCostsDashboard(): Promise<CostsDashboard> {
  try {
    const raw = await request<any>('GET', '/api/v1/costs/dashboard');
    return {
      daily: normPeriod(raw?.daily),
      weekly: normPeriod(raw?.weekly),
      monthly: normPeriod(raw?.monthly),
      all_time: normPeriod(raw?.all_time),
    };
  } catch {
    return { daily: { ...ZERO_PERIOD }, weekly: { ...ZERO_PERIOD }, monthly: { ...ZERO_PERIOD }, all_time: { ...ZERO_PERIOD } };
  }
}

// ── Skills ────────────────────────────────────────────────────────────
export interface SkillInfo {
  name: string;
  description: string;
}

export async function getSkills(): Promise<SkillInfo[]> {
  try {
    return await request<SkillInfo[]>('GET', '/api/v1/skills');
  } catch {
    return [];
  }
}

// ── Style ────────────────────────────────────────────────────────
export interface StyleInfo {
  style: string;
}

export async function getStyle(): Promise<StyleInfo> {
  try {
    const raw = await request<any>('GET', '/api/v1/style');
    // Backend returns { active_style: "..." } not { style: "..." }
    return { style: raw?.style || raw?.active_style || '' };
  } catch {
    return { style: '' };
  }
}

export async function setStyle(style: string): Promise<void> {
  await request<void>('POST', '/api/v1/style', { style });
}

export async function clearStyle(): Promise<void> {
  await request<void>('DELETE', '/api/v1/style');
}

// ── Resources ─────────────────────────────────────────────────────────
export interface ResourceInfo {
  id: string;
  resource_type: string;
  status: string;
}

export async function getResources(): Promise<ResourceInfo[]> {
  try {
    return await request<ResourceInfo[]>('GET', '/api/v1/resources');
  } catch {
    return [];
  }
}
