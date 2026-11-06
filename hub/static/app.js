(() => {
  const csrf = () => document.querySelector('meta[name="csrf-token"]')?.content || '';
  const toast = (message, kind = 'good') => {
    const node = document.getElementById('toast');
    if (!node) return;
    node.textContent = message;
    node.className = `toast show ${kind}`;
    setTimeout(() => { node.className = 'toast'; }, 4200);
  };
  const jsonRequest = async (url, options = {}) => {
    const response = await fetch(url, {
      ...options,
      headers: {'Content-Type': 'application/json', 'X-CSRF-Token': csrf(), ...(options.headers || {})},
    });
    const data = response.status === 204 ? {} : await response.json();
    if (!response.ok) throw new Error(data?.error?.message || data?.detail || 'The action failed');
    return data;
  };
  document.querySelectorAll('[data-copy-target]').forEach(button => {
    button.addEventListener('click', async () => {
      const node = document.querySelector(button.dataset.copyTarget);
      if (!node) return;
      await navigator.clipboard.writeText(`${node.value ?? node.textContent}`.trim());
      toast('Copied to clipboard');
    });
  });
  document.querySelectorAll('[data-download-target]').forEach(button => {
    button.addEventListener('click', () => {
      const node = document.querySelector(button.dataset.downloadTarget);
      if (!node) return;
      const blob = new Blob([`${node.value ?? node.textContent}`.trim() + '\n'], {type: 'text/plain;charset=utf-8'});
      const link = document.createElement('a');
      link.href = URL.createObjectURL(blob); link.download = button.dataset.downloadName || 'codeintel.txt';
      link.click(); setTimeout(() => URL.revokeObjectURL(link.href), 1000);
      toast('File ready to download');
    });
  });
  const promptButton = document.getElementById('build-agent-prompt');
  if (promptButton) {
    const promptNode = document.getElementById('universal-prompt-template');
    const originalPrompt = promptNode.value;
    promptButton.addEventListener('click', () => {
      const workerToken = document.getElementById('prompt-worker-token').value.trim();
      const projectToken = document.getElementById('prompt-project-token').value.trim();
      promptNode.value = originalPrompt
        .replaceAll('<HOST_INSTALL_TOKEN>', workerToken || 'NOT_PROVIDED_LOCAL_ONLY')
        .replaceAll('<PROJECT_ACCESS_TOKEN>', projectToken || 'NOT_PROVIDED');
      toast('The prompt contains the supplied credentials. Store it securely.');
    });
  }
  const tokenGuide = (token, tokenType, scopeId) => {
    const warning = 'SECURITY: this file contains a plaintext secret. Do not commit or log it. Rotate/revoke it after use.';
    if (tokenType === 'WORKER_ENROLLMENT') return `${warning}\n\nInstall Code Intelligence Worker:\n\ncurl -fsSL ${location.origin}/downloads/install.sh | sh -s -- \\\n  --hub ${location.origin} \\\n  --install-token '${token}' \\\n  --clients codex,claude,hermes\n\nThis token is used once. Agents do not need individual tokens.`;
    if (tokenType === 'PROJECT_AGENT') return `${warning}\n\nProject Access for ${scopeId || '<PROJECT_ID>'}:\n{\n  "url": "${location.origin}/mcp",\n  "headers": {"Authorization": "Bearer ${token}"}\n}\n\nThe same Project Access works for authorized Codex, Claude, Hermes and Generic Agents. Local Code Intelligence does not require it.`;
    return `${warning}\n\nToken type: ${tokenType}\nScope: ${scopeId || 'GLOBAL'}\nToken: ${token}\nHub: ${location.origin}\n\nUse only for the declared scope and rotate/revoke from Hub > Tokens.`;
  };
  const remoteReaderPrompt = document.getElementById('remote-reader-prompt');
  const originalRemoteReaderPrompt = remoteReaderPrompt?.value || '';
  const insertRemoteReaderToken = token => {
    if (!remoteReaderPrompt || !originalRemoteReaderPrompt) return;
    remoteReaderPrompt.value = originalRemoteReaderPrompt.replaceAll('<PROJECT_ACCESS_TOKEN>', token);
  };
  const showTokenResult = (data, fallbackType = null, fallbackScope = null) => {
    const result = document.getElementById('token-result');
    if (!result) return;
    result.querySelector('code').textContent = data.token;
    result.classList.remove('hidden');
    if ((data.token_type || fallbackType) === 'PROJECT_AGENT') insertRemoteReaderToken(data.token);
    const guide = document.getElementById('token-guide');
    if (guide) {
      guide.textContent = tokenGuide(data.token, data.token_type || fallbackType, data.scope_id || fallbackScope);
      document.getElementById('token-guide-wrap').classList.remove('hidden');
    }
  };
  const createForm = document.getElementById('token-create-form');
  if (createForm) createForm.addEventListener('submit', async event => {
    event.preventDefault();
    const form = new FormData(createForm);
    const tokenType = form.get('token_type');
    const scopeId = form.get('scope_id') || null;
    const isProject = Boolean(scopeId);
    const scopes = tokenType === 'PROJECT_AGENT' && scopeId
      ? [`project:${scopeId}:read`, `project:${scopeId}:context_read`, `project:${scopeId}:context_write`, `project:${scopeId}:health_read`, `project:${scopeId}:quality_read`, `project:${scopeId}:findings_read`, `project:${scopeId}:history_read`, `project:${scopeId}:agent_setup_read`, `project:${scopeId}:session_log`, `project:${scopeId}:live_analysis`, `project:${scopeId}:instance_bind`]
      : tokenType === 'WORKER_ENROLLMENT' ? ['worker:enroll']
      : tokenType === 'ADMIN_API' ? ['admin'] : ['hub:read'];
    try {
      const data = await jsonRequest('/api/v1/tokens', {method: 'POST', body: JSON.stringify({
        label: form.get('label'), token_type: tokenType,
        scope_type: isProject ? 'PROJECT' : 'GLOBAL', scope_id: scopeId, scopes,
        expires_in_sec: tokenType === 'WORKER_ENROLLMENT' ? 900 : null,
        one_time: tokenType === 'WORKER_ENROLLMENT',
      })});
      showTokenResult(data, tokenType, scopeId);
      toast('Token created. Copy it now.');
    } catch (error) { toast(error.message, 'bad'); }
  });
  let pendingAction = null;
  document.querySelectorAll('[data-project-access-create]').forEach(button => button.addEventListener('click', () => {
    pendingAction = {kind: 'project-access', action: 'rotate', projectId: button.dataset.projectId};
    document.getElementById('reauth-dialog')?.showModal();
  }));
  document.querySelectorAll('[data-project-access-test]').forEach(button => button.addEventListener('click', async () => {
    try {
      const data = await jsonRequest(`/api/v1/projects/${button.dataset.projectId}/access/test`, {method: 'POST'});
      toast(data.usable ? 'Project Access is valid' : 'Project Access is missing or revoked', data.usable ? 'good' : 'bad');
    } catch (error) { toast(error.message, 'bad'); }
  }));
  document.querySelectorAll('[data-project-access-revoke]').forEach(button => button.addEventListener('click', async () => {
    if (!confirm('Revoke Project Access to the Hub? Local work will continue.')) return;
    try { await jsonRequest(`/api/v1/projects/${button.dataset.projectId}/access/revoke`, {method: 'POST'}); location.reload(); }
    catch (error) { toast(error.message, 'bad'); }
  }));
  document.querySelectorAll('[data-host-access-action]').forEach(button => button.addEventListener('click', async () => {
    if (button.dataset.hostAccessAction === 'revoke') {
      if (!confirm('Disable creation/linking of new projects? Existing projects will continue working.')) return;
      try { await jsonRequest(`/api/v1/hosts/${button.dataset.hostId}/access/revoke`, {method: 'POST'}); location.reload(); }
      catch (error) { toast(error.message, 'bad'); }
      return;
    }
    pendingAction = {kind: 'host-access', action: 'rotate', hostId: button.dataset.hostId};
    document.getElementById('reauth-dialog')?.showModal();
  }));
  const hostInstallButton = document.getElementById('create-host-install-token');
  if (hostInstallButton) hostInstallButton.addEventListener('click', async () => {
    try {
      hostInstallButton.disabled = true;
      const data = await jsonRequest('/api/v1/host-install-tokens', {method: 'POST'});
      const result = document.getElementById('host-install-result');
      result.querySelector('code').textContent = [
        `curl -fsSL ${location.origin}/downloads/install.sh | sh -s -- \\`,
        `  --hub ${location.origin} \\`,
        `  --install-token '${data.install_token}' \\`,
        '  --clients codex,claude,hermes',
      ].join('\n');
      result.querySelector('small').textContent = `Valid until ${data.expires_at}; displayed once only.`;
      result.classList.remove('hidden');
      toast('Host Install Token and installation command created.');
    } catch (error) { toast(error.message, 'bad'); }
    finally { hostInstallButton.disabled = false; }
  });
  document.querySelectorAll('[data-token-action]').forEach(button => button.addEventListener('click', async () => {
    const action = button.dataset.tokenAction;
    const id = button.dataset.tokenId;
    if (action === 'revoke') {
      if (!confirm('Revoke this token? The connection will stop working immediately.')) return;
      try { await jsonRequest(`/api/v1/tokens/${id}`, {method: 'DELETE'}); location.reload(); }
      catch (error) { toast(error.message, 'bad'); }
      return;
    }
    pendingAction = {kind: 'token', action, id, tokenType: button.dataset.tokenType, scopeId: button.dataset.scopeId};
    document.getElementById('reauth-dialog')?.showModal();
  }));
  const confirmReauth = document.getElementById('reauth-confirm');
  if (confirmReauth) confirmReauth.addEventListener('click', async event => {
    event.preventDefault();
    const password = document.getElementById('reauth-password').value;
    try {
      await jsonRequest('/api/v1/auth/re-auth', {method: 'POST', body: JSON.stringify({password})});
      let data;
      if (pendingAction.kind === 'host-access') {
        data = await jsonRequest(`/api/v1/hosts/${pendingAction.hostId}/access/rotate`, {method: 'POST'});
        showTokenResult(data, 'HOST_ACCESS', pendingAction.hostId);
      } else if (pendingAction.kind === 'project-access') {
        data = await jsonRequest(`/api/v1/projects/${pendingAction.projectId}/access/rotate`, {method: 'POST'});
        showTokenResult(data, 'PROJECT_AGENT', pendingAction.projectId);
      } else {
        data = await jsonRequest(`/api/v1/tokens/${pendingAction.id}/${pendingAction.action}`, {method: 'POST'});
        showTokenResult(data, pendingAction.tokenType, pendingAction.scopeId);
      }
      document.getElementById('reauth-dialog').close();
      toast(pendingAction.action === 'rotate' ? 'Access rotated; the old credential is invalid.' : 'Token revealed.');
    } catch (error) { toast(error.message, 'bad'); }
  });
})();
