import { useCallback, useEffect, useMemo, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window'

const appWindow = getCurrentWindow()
const providers = [
  { id: 'openai', name: 'OpenAI', baseUrl: 'https://api.openai.com/v1', model: 'gpt-5.4-mini' },
  { id: 'gemini', name: 'Google Gemini', baseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai', model: 'gemini-3-flash-preview' },
  { id: 'deepseek', name: 'DeepSeek', baseUrl: 'https://api.deepseek.com/v1', model: 'deepseek-v4-flash' },
  { id: 'doubao', name: '火山方舟 / 豆包', baseUrl: 'https://ark.cn-beijing.volces.com/api/v3', model: 'doubao-seed-2-0-lite-260215' },
  { id: 'siliconflow', name: '硅基流动', baseUrl: 'https://api.siliconflow.cn/v1', model: 'Qwen/Qwen2.5-7B-Instruct' },
  { id: 'custom', name: '自定义', baseUrl: '', model: '' },
]

const aiChannels = [
  { id: 'primary', label: 'AI 1', configKey: 'ai', inputKey: 'ai', fallbackProviderId: 'openai' },
  { id: 'secondary', label: 'AI 2', configKey: 'ai_secondary', inputKey: 'aiSecondary', fallbackProviderId: 'deepseek' },
]

const settingsTabs = [
  { key: 'ai', label: 'AI 设置', icon: '🤖' },
  { key: 'ocr', label: 'OCR 设置', icon: '📝' },
  { key: 'screenshot', label: '截图设置', icon: '◱' },
  { key: 'history', label: '答题记录', icon: '📒' },
]

function errorText(error) {
  return error?.message ?? String(error)
}

function singleLine(value) {
  return String(value || '').replace(/\s+/g, ' ').trim()
}

function firstAnswerLine(value) {
  const text = singleLine(value)
  return text.split(/[。！？；，,.!?;]/)[0]?.trim() || text
}

function formatMs(value) {
  if (value === null || value === undefined) return '-'
  return `${value} ms`
}

function poemTitle(item) {
  return item?.poem?.title ?? item?.title ?? '无题'
}

function poemAuthor(item) {
  return item?.poem?.author ?? item?.author ?? '佚名'
}

function poemText(item) {
  return item?.poem?.text ?? item?.text ?? ''
}

function poemMatchedClause(item) {
  return item?.matched_clause ?? item?.matchedClause ?? ''
}

function defaultAiForm(providerId = 'openai') {
  const provider = providers.find((item) => item.id === providerId) ?? providers[0]
  return {
    providerId: provider.id,
    customBaseUrl: '',
    apiKey: '',
    model: provider.model,
    modelOptions: [],
    loadingModels: false,
    usePrimaryEndpoint: false,
  }
}

function aiFormFromConfig(config, fallbackProviderId) {
  const savedProvider = providers.find((item) => item.id === config?.provider)
  const provider = savedProvider ?? providers.find((item) => item.id === fallbackProviderId) ?? providers[0]
  return {
    providerId: savedProvider ? savedProvider.id : config?.provider ? 'custom' : provider.id,
    customBaseUrl: config?.base_url || '',
    apiKey: '',
    model: config?.model || provider.model,
    modelOptions: [],
    loadingModels: false,
    usePrimaryEndpoint: Boolean(config?.use_primary_endpoint ?? config?.usePrimaryEndpoint),
  }
}

function aiResults(answerResult) {
  if (Array.isArray(answerResult?.ai_answers)) return answerResult.ai_answers
  if (answerResult?.ai_answer || answerResult?.ai_error) {
    return [{
      channel: 'primary',
      label: 'AI 1',
      provider: '',
      model: '',
      answer: answerResult.ai_answer?.answer || null,
      error: answerResult.ai_error || null,
      elapsed_ms: answerResult.ai_answer?.elapsed_ms || 0,
    }]
  }
  return []
}

function mergeAiChannelResult(answerResult, channelResult) {
  if (!answerResult) return answerResult
  const orderedChannels = ['primary', 'secondary']
  const mergedAnswers = [
    ...aiResults(answerResult).filter((item) => item.channel !== channelResult.channel),
    channelResult,
  ].sort((left, right) => orderedChannels.indexOf(left.channel) - orderedChannels.indexOf(right.channel))
  const firstAnswer = mergedAnswers.find((item) => item.answer)
  const allFinished = mergedAnswers.length >= aiChannels.length
  const aiError = firstAnswer
    ? null
    : allFinished
      ? mergedAnswers.map((item) => `${item.label || 'AI'}: ${item.error || '无返回'}`).join('; ')
      : answerResult.ai_error || null
  return {
    ...answerResult,
    ai_answers: mergedAnswers,
    ai_answer: firstAnswer ? { answer: firstAnswer.answer, elapsed_ms: firstAnswer.elapsed_ms } : null,
    ai_error: aiError,
    timings: {
      ...answerResult.timings,
      ai_ms: mergedAnswers.length > 0 ? Math.max(...mergedAnswers.map((item) => item.elapsed_ms || 0)) : answerResult.timings?.ai_ms,
    },
  }
}

export default function App() {
  const [currentView, setCurrentView] = useState('main')
  const [settingsTab, setSettingsTab] = useState('ai')
  const [aiChannelId, setAiChannelId] = useState('primary')
  const [aiForms, setAiForms] = useState(() => ({
    primary: defaultAiForm('openai'),
    secondary: defaultAiForm('deepseek'),
  }))
  const [status, setStatus] = useState('React 版启动中...')
  const [savingConfig, setSavingConfig] = useState(false)
  const [configInfo, setConfigInfo] = useState(null)
  const [baiduApiKey, setBaiduApiKey] = useState('')
  const [baiduSecretKey, setBaiduSecretKey] = useState('')
  const [ocrText, setOcrText] = useState('')
  const [ocrImageBase64, setOcrImageBase64] = useState('')
  const [runningOcr, setRunningOcr] = useState(false)
  const [selectorVisible, setSelectorVisible] = useState(false)
  const [screenshotCapturing, setScreenshotCapturing] = useState(false)
  const [answerResult, setAnswerResult] = useState(null)
  const [answering, setAnswering] = useState(false)
  const [poetryQuery, setPoetryQuery] = useState('')
  const [poetryResults, setPoetryResults] = useState([])
  const [detailExpanded, setDetailExpanded] = useState(false)

  const aiChannel = aiChannels.find((item) => item.id === aiChannelId) ?? aiChannels[0]
  const activeAiForm = aiForms[aiChannel.id] ?? defaultAiForm(aiChannel.fallbackProviderId)
  const primaryAiForm = aiForms.primary ?? defaultAiForm('openai')
  const usesPrimaryEndpoint = aiChannel.id === 'secondary' && activeAiForm.usePrimaryEndpoint
  const endpointAiForm = usesPrimaryEndpoint ? primaryAiForm : activeAiForm
  const selectedProvider = useMemo(
    () => providers.find((item) => item.id === endpointAiForm.providerId) ?? providers[0],
    [endpointAiForm.providerId],
  )
  const baseUrl = selectedProvider.id === 'custom' ? endpointAiForm.customBaseUrl : selectedProvider.baseUrl
  const endpointConfigKey = usesPrimaryEndpoint ? 'ai' : aiChannel.configKey
  const endpointSavedAiConfig = configInfo?.[endpointConfigKey]
  const hasSavedAiKey = Boolean(endpointSavedAiConfig?.has_api_key)
  const hasAnySavedAiKey = Boolean(configInfo?.ai?.has_api_key || configInfo?.ai_secondary?.has_api_key)
  const hasSavedBaiduKey = Boolean(configInfo?.baidu_ocr?.has_api_key && configInfo?.baidu_ocr?.has_secret_key)
  const hasOcrCredential = Boolean(hasSavedBaiduKey || (baiduApiKey.trim() && baiduSecretKey.trim()))
  const busy = screenshotCapturing || runningOcr || answering
  const canFetchModels = Boolean((endpointAiForm.apiKey.trim() || hasSavedAiKey) && baseUrl.trim() && !activeAiForm.loadingModels)
  const captureButtonText = busy ? '处理中...' : selectorVisible ? '截图并识别' : '选择截图区域'
  const ocrSingleLine = singleLine(ocrText || '识别的文本将显示在这里...')
  const firstPoetryResult = poetryResults[0] ?? null
  const answerAiResults = aiResults(answerResult)
  const firstAiResult = answerAiResults.find((item) => item.answer)
  const bestAnswer = poemMatchedClause(firstPoetryResult) || firstAnswerLine(firstAiResult?.answer || answerResult?.ai_answer?.answer || '')
  const bestAnswerSource = poemMatchedClause(firstPoetryResult)
    ? '知识库'
    : firstAiResult?.answer
      ? firstAiResult.label || 'AI'
      : answerResult?.ai_error
        ? '错误'
        : '等待'
  const bestAnswerSubtext = firstPoetryResult
    ? `《${poemTitle(firstPoetryResult)}》${poemAuthor(firstPoetryResult)}${firstAiResult?.answer ? ` · AI参考：${firstAnswerLine(firstAiResult.answer)}` : ''}`
    : firstAiResult?.answer
      ? `${firstAiResult.label || 'AI'} 回答 · 双击复制`
      : answering
        ? '正在等待 AI 1 / AI 2 / 诗词库返回'
        : '暂无答案'
  const timingSummary = answerResult?.timings
    ? `诗词 ${formatMs(answerResult.timings.poetry_ms)} / AI ${formatMs(answerResult.timings.ai_ms)} / 总 ${formatMs(answerResult.timings.total_ms)}`
    : status
  const answerTypeText = answerResult?.question_type === 'poetry' ? '诗词组字题' : answerResult ? '普通常识题' : '等待答题'
  const appReadyText = `${hasAnySavedAiKey ? 'AI已配' : 'AI未配'} · ${hasSavedBaiduKey ? 'OCR已配' : 'OCR未配'}`

  const resizeForCurrentView = useCallback(async () => {
    try {
      await appWindow.setSize(new LogicalSize(currentView === 'settings' ? 820 : 760, currentView === 'settings' ? 640 : detailExpanded ? 560 : 330))
    } catch {
      // Keep UI usable if WebView denies resizing.
    }
  }, [currentView, detailExpanded])

  useEffect(() => {
    resizeForCurrentView()
  }, [resizeForCurrentView])

  useEffect(() => {
    loadHealth()
    loadConfig()

    let removeCaptured
    let removeClosed
    listen('screenshot-captured', (event) => handleSelectorCaptured(event.payload)).then((fn) => {
      removeCaptured = fn
    })
    listen('screenshot-selector-closed', () => {
      setSelectorVisible(false)
      setStatus('截图区域已关闭')
    }).then((fn) => {
      removeClosed = fn
    })
    return () => {
      removeCaptured?.()
      removeClosed?.()
    }
  }, [])

  async function loadHealth() {
    try {
      setStatus(await invoke('health'))
    } catch (error) {
      setStatus(`宿主未连接：${errorText(error)}`)
    }
  }

  async function loadConfig() {
    try {
      applyConfig(await invoke('get_config'))
    } catch (error) {
      setStatus(`读取配置失败：${errorText(error)}`)
    }
  }

  function updateAiForm(channelId, updater) {
    setAiForms((forms) => ({
      ...forms,
      [channelId]: typeof updater === 'function'
        ? updater(forms[channelId] ?? defaultAiForm(aiChannels.find((item) => item.id === channelId)?.fallbackProviderId))
        : { ...(forms[channelId] ?? defaultAiForm()), ...updater },
    }))
  }

  function updateActiveAiForm(patch) {
    updateAiForm(aiChannel.id, patch)
  }

  function applyConfig(config) {
    setConfigInfo(config)
    setAiForms({
      primary: aiFormFromConfig(config.ai, 'openai'),
      secondary: aiFormFromConfig(config.ai_secondary, 'deepseek'),
    })
  }

  function selectProvider(id) {
    const provider = providers.find((item) => item.id === id) ?? providers[0]
    updateActiveAiForm({
      providerId: provider.id,
      model: provider.model,
      modelOptions: [],
    })
  }

  async function saveAiConfig() {
    setSavingConfig(true)
    try {
      const ai = usesPrimaryEndpoint
        ? {
            model: activeAiForm.model,
            usePrimaryEndpoint: true,
          }
        : {
            provider: selectedProvider.id,
            model: activeAiForm.model,
            baseUrl,
            usePrimaryEndpoint: false,
          }
      if (!usesPrimaryEndpoint && activeAiForm.apiKey.trim()) ai.apiKey = activeAiForm.apiKey.trim()
      applyConfig(await invoke('save_config', { input: { [aiChannel.inputKey]: ai } }))
      updateActiveAiForm({ apiKey: '' })
      setStatus(`${aiChannel.label} 配置已保存`)
    } catch (error) {
      setStatus(`保存 ${aiChannel.label} 配置失败：${errorText(error)}`)
    } finally {
      setSavingConfig(false)
    }
  }

  async function saveOcrConfig() {
    setSavingConfig(true)
    try {
      const baiduOcr = {}
      if (baiduApiKey.trim()) baiduOcr.apiKey = baiduApiKey.trim()
      if (baiduSecretKey.trim()) baiduOcr.secretKey = baiduSecretKey.trim()
      applyConfig(await invoke('save_config', { input: { baiduOcr } }))
      setBaiduApiKey('')
      setBaiduSecretKey('')
      setStatus('百度 OCR 配置已保存')
    } catch (error) {
      setStatus(`保存 OCR 配置失败：${errorText(error)}`)
    } finally {
      setSavingConfig(false)
    }
  }

  async function fetchModels() {
    if (!baseUrl.trim()) {
      setStatus('请先填写 API URL')
      return
    }
    updateActiveAiForm({ loadingModels: true })
    setStatus(`${aiChannel.label} 正在通过 /models 获取模型列表...`)
    try {
      const models = await invoke('list_ai_models', {
        provider: selectedProvider.id,
        baseUrl,
        apiKey: endpointAiForm.apiKey || null,
      })
      updateActiveAiForm((form) => ({
        ...form,
        modelOptions: models,
        model: form.model || models[0]?.id || '',
      }))
      setStatus(models.length > 0 ? `已获取 ${models.length} 个模型` : '接口返回为空，可手动输入模型名')
    } catch (error) {
      setStatus(`获取模型失败：${errorText(error)}`)
    } finally {
      updateActiveAiForm({ loadingModels: false })
    }
  }

  async function toggleScreenshotSelector() {
    try {
      if (selectorVisible) {
        await invoke('hide_screenshot_selector')
        setSelectorVisible(false)
        setStatus('截图区域已隐藏')
        return
      }
      await invoke('show_screenshot_selector')
      setSelectorVisible(true)
      setStatus('已打开截图区域：中间拖动位置，边缘/四角缩放')
    } catch (error) {
      setStatus(`切换截图区域失败：${errorText(error)}`)
    }
  }

  async function captureAndRecognize() {
    if (!hasOcrCredential) {
      setStatus('请先在设置页保存百度 OCR API Key 和 Secret Key')
      setCurrentView('settings')
      setSettingsTab('ocr')
      return
    }
    if (!selectorVisible) {
      await toggleScreenshotSelector()
      setStatus('已打开截图区域：调整后按 Enter，或再次点击“截图并识别”')
      return
    }

    setScreenshotCapturing(true)
    setStatus('正在截图...')
    try {
      const result = await invoke('capture_screenshot_from_selector', { closeBeforeCapture: true })
      setSelectorVisible(false)
      setOcrImageBase64(result.data)
      await recognizeAndAnswer(result.data)
    } catch (error) {
      setStatus(`截图失败：${errorText(error)}`)
    } finally {
      setScreenshotCapturing(false)
    }
  }

  async function handleSelectorCaptured(payload) {
    if (!payload?.data) return
    setSelectorVisible(false)
    setOcrImageBase64(payload.data)
    await recognizeAndAnswer(payload.data)
  }

  async function recognizeAndAnswer(imageBase64) {
    setRunningOcr(true)
    setAnswerResult(null)
    setPoetryResults([])
    setStatus('正在调用百度 OCR...')
    try {
      const result = await invoke('baidu_ocr_base64', {
        imageBase64,
        apiKey: baiduApiKey || null,
        secretKey: baiduSecretKey || null,
      })
      setOcrText(result.text)
      setPoetryQuery(result.text)
      setStatus(`OCR 完成：${result.lines.length} 行文本，开始答题...`)
      await answerFromText(result.text)
    } catch (error) {
      setStatus(`OCR 失败：${errorText(error)}`)
    } finally {
      setRunningOcr(false)
    }
  }

  async function answerFromText(questionText) {
    const text = questionText.trim()
    if (!text) {
      setStatus('OCR 未识别到有效文本')
      return
    }
    setAnswering(true)
    setStatus('正在答题：普通题并行调用 AI 1 / AI 2，诗词题优先知识库...')
    try {
      const result = await invoke('answer_question', { questionText: text })
      setAnswerResult(result)
      setPoetryResults(result.poetry_results || [])
      if (result.poetry_query) setPoetryQuery(result.poetry_query)
      if (result.question_type === 'poetry' && result.poetry_results.length > 0) {
        setStatus(`诗词题完成：诗词库 ${result.poetry_results.length} 条，AI 已跳过`)
        recordHistoryEntry({
          question_type: result.question_type,
          question_text: text,
          poetry_query: result.poetry_query,
          poetry_results: result.poetry_results,
          ai_answers: [],
          timings: result.timings,
        })
        return
      }

      setStatus('知识库未命中，AI 1 / AI 2 正在并发答题...')
      const channelResults = await Promise.all(
        aiChannels.map((channel) => invoke('answer_ai_channel', {
          questionText: text,
          channel: channel.id,
        }).then((aiResult) => {
          setAnswerResult((current) => mergeAiChannelResult(current, aiResult))
          setStatus(`${aiResult.label} ${aiResult.answer ? '已返回' : `出错：${aiResult.error || '无返回'}`}`)
          return aiResult
        }).catch((error) => {
          const aiResult = {
            channel: channel.id,
            label: channel.label,
            provider: '',
            model: '',
            answer: null,
            error: errorText(error),
            elapsed_ms: 0,
          }
          setAnswerResult((current) => mergeAiChannelResult(current, aiResult))
          setStatus(`${channel.label} 出错：${errorText(error)}`)
          return aiResult
        })),
      )
      const aiSuccessCount = channelResults.filter((item) => item.answer).length
      const aiErrorCount = channelResults.length - aiSuccessCount
      setStatus(`AI 答题完成：成功 ${aiSuccessCount} / 出错 ${aiErrorCount}`)
      recordHistoryEntry({
        question_type: result.question_type,
        question_text: text,
        poetry_query: result.poetry_query,
        poetry_results: result.poetry_results,
        ai_answers: channelResults,
        timings: {
          ...result.timings,
          ai_channel_ms: channelResults.map((r) => ({ channel: r.channel, ms: r.elapsed_ms })),
        },
      })
    } catch (error) {
      setStatus(`答题失败：${errorText(error)}`)
    } finally {
      setAnswering(false)
    }
  }

  function clearResults() {
    setOcrImageBase64('')
    setOcrText('')
    setAnswerResult(null)
    setPoetryQuery('')
    setPoetryResults([])
    setStatus('已清空，等待下一题')
  }

  async function recordHistoryEntry(entry) {
    try {
      await invoke('record_history', { entry })
    } catch (error) {
      console.warn('record_history failed:', error)
    }
  }

  async function toggleHistoryEnabled(next) {
    setSavingConfig(true)
    try {
      applyConfig(await invoke('save_config', { input: { history: { enabled: next } } }))
      setStatus(next ? '答题记录已开启' : '答题记录已关闭')
    } catch (error) {
      setStatus(`保存记录开关失败：${errorText(error)}`)
    } finally {
      setSavingConfig(false)
    }
  }

  async function copyBestAnswer() {
    if (!bestAnswer) return
    try {
      await navigator.clipboard.writeText(bestAnswer)
      setStatus(`已复制：${bestAnswer}`)
    } catch {
      setStatus('复制失败，请手动复制')
    }
  }

  async function quitApp() {
    try {
      await invoke('quit_app')
    } catch {
      await appWindow.close()
    }
  }

  return (
    <div className="app-shell">
      <header className="title-bar" data-tauri-drag-region>
        <div className="title-left">
          <span className="app-name">殿试答题器</span>
          <span className="version">React v5.0</span>
          <span className="ready-pill">{appReadyText}</span>
        </div>
        <nav className="title-right">
          <button className={currentView === 'main' ? 'nav active' : 'nav'} type="button" onClick={() => setCurrentView('main')}>答题</button>
          <button className={currentView === 'settings' ? 'nav active' : 'nav'} type="button" onClick={() => setCurrentView('settings')}>设置</button>
          <button className="window-btn" type="button" onClick={() => appWindow.minimize()}>—</button>
          <button className="window-btn close" type="button" onClick={quitApp}>×</button>
        </nav>
      </header>

      {currentView === 'main' ? (
        <main className="answer-page">
          <section className="quick-card">
            <div className="action-row">
              <button type="button" className="neo secondary" disabled={busy} onClick={toggleScreenshotSelector}>
                {selectorVisible ? '隐藏截图区域' : '打开截图区域'}
              </button>
              <button type="button" className="neo primary" disabled={busy} onClick={captureAndRecognize}>
                {captureButtonText}
              </button>
              <button type="button" className="neo light" disabled={busy && !answerResult && !ocrText} onClick={clearResults}>清空</button>
            </div>

            <div className="line-box">
              <span>OCR</span>
              <p title={ocrText}>{ocrSingleLine}</p>
            </div>

            <div className="best-box">
              <div className="best-main">
                <span>最佳答案</span>
                <strong className={bestAnswer ? '' : 'empty'} onDoubleClick={copyBestAnswer}>{bestAnswer || (busy ? '处理中...' : '等待答题')}</strong>
                <em>{bestAnswerSource}</em>
              </div>
              <p title={bestAnswerSubtext}>{bestAnswerSubtext}</p>
            </div>

            <div className="status-row">
              <span>{answerTypeText}</span>
              <p>{timingSummary}</p>
              <button type="button" onClick={() => setDetailExpanded((value) => !value)}>{detailExpanded ? '收起详情' : '展开详情'}</button>
            </div>
          </section>

          {detailExpanded && (
            <section className="detail-grid">
              <article className="result-card poetry">
                <h2>📚 知识库匹配</h2>
                {poetryResults.length === 0 ? (
                  <p className="empty-text">{answerResult?.question_type === 'general' ? '普通题无需诗词库匹配。' : '暂无匹配结果'}</p>
                ) : (
                  <div className="result-list">
                    {poetryResults.slice(0, 3).map((item, index) => (
                      <div className="result-item" key={item.poem?.id ?? index}>
                        <span>《{poemTitle(item)}》</span>
                        <strong>{poemMatchedClause(item) || poemText(item)}</strong>
                        {item.score !== undefined && <em>{Math.round(item.score * 100)}%</em>}
                      </div>
                    ))}
                  </div>
                )}
              </article>
              <article className="result-card ai">
                <h2>🤖 AI 回答</h2>
                {answerAiResults.length > 0 ? (
                  <div className="result-list">
                    {answerAiResults.map((item) => (
                      <div className={item.answer ? 'result-item' : 'result-item error'} key={item.channel || item.label}>
                        <span>{item.label || 'AI'}</span>
                        <strong>{item.answer || item.error || '无返回'}</strong>
                        <em>{(item.elapsed_ms / 1000).toFixed(2)}s</em>
                      </div>
                    ))}
                  </div>
                ) : answering ? (
                  <p className="empty-text">AI 1 / AI 2 回答中...</p>
                ) : answerResult?.question_type === 'poetry' && poetryResults.length > 0 ? (
                  <p className="empty-text">诗词库已命中，已跳过 AI</p>
                ) : (
                  <p className="empty-text">AI 回答将显示在这里</p>
                )}
              </article>
            </section>
          )}
        </main>
      ) : (
        <main className="settings-page">
          <aside className="settings-sidebar">
            <div className="settings-logo">
              <strong>设置</strong>
              <span>React v5.0</span>
            </div>
            {settingsTabs.map((tab) => (
              <button
                key={tab.key}
                type="button"
                className={settingsTab === tab.key ? 'settings-tab active' : 'settings-tab'}
                onClick={() => setSettingsTab(tab.key)}
              >
                <span>{tab.icon}</span>
                {tab.label}
              </button>
            ))}
            <button type="button" className="neo primary back-main" onClick={() => setCurrentView('main')}>返回答题</button>
          </aside>

          <section className="settings-card">
            {settingsTab === 'ai' && (
              <>
                <header className="card-title ai-title">
                  <h2>🤖 AI 设置</h2>
                  <span>双通道并发 · 单独超时 · 互不影响</span>
                </header>
                <div className="channel-tabs">
                  {aiChannels.map((channel) => {
                    const channelConfig = configInfo?.[channel.configKey]
                    const isReusingPrimary = channel.id === 'secondary' && Boolean(channelConfig?.use_primary_endpoint ?? channelConfig?.usePrimaryEndpoint)
                    return (
                      <button
                        key={channel.id}
                        type="button"
                        className={aiChannelId === channel.id ? 'active' : ''}
                        onClick={() => setAiChannelId(channel.id)}
                      >
                        <span>{channel.label}</span>
                        <em>{isReusingPrimary ? '复用 AI 1' : channelConfig?.has_api_key ? '已配置' : '未配置'}</em>
                      </button>
                    )
                  })}
                </div>
                {aiChannel.id === 'secondary' && (
                  <label className="inline-check">
                    <input
                      type="checkbox"
                      checked={Boolean(activeAiForm.usePrimaryEndpoint)}
                      onChange={(event) => updateActiveAiForm({ usePrimaryEndpoint: event.target.checked })}
                    />
                    复用 AI 1 的 API URL 和 API Key
                  </label>
                )}
                <div className="form-grid">
                  <label>
                    服务商{usesPrimaryEndpoint ? '（来自 AI 1）' : ''}
                    <select value={endpointAiForm.providerId} onChange={(event) => selectProvider(event.target.value)} disabled={usesPrimaryEndpoint}>
                      {providers.map((provider) => (
                        <option key={provider.id} value={provider.id}>{provider.name}</option>
                      ))}
                    </select>
                  </label>
                  <label>
                    API URL{usesPrimaryEndpoint ? '（来自 AI 1）' : ''}
                    {selectedProvider.id === 'custom' ? (
                      <input value={endpointAiForm.customBaseUrl} onChange={(event) => updateActiveAiForm({ customBaseUrl: event.target.value })} placeholder="https://example.com/v1" readOnly={usesPrimaryEndpoint} />
                    ) : (
                      <input value={baseUrl} readOnly />
                    )}
                  </label>
                  <label>
                    API Key
                    <input
                      type="password"
                      autoComplete="off"
                      value={usesPrimaryEndpoint ? endpointAiForm.apiKey : activeAiForm.apiKey}
                      onChange={(event) => updateActiveAiForm({ apiKey: event.target.value })}
                      disabled={usesPrimaryEndpoint}
                      placeholder={endpointSavedAiConfig?.has_api_key ? `已保存：${endpointSavedAiConfig.api_key_redacted}` : usesPrimaryEndpoint ? '使用 AI 1 的 API Key' : 'sk-...'}
                    />
                  </label>
                  <label>
                    模型
                    <input value={activeAiForm.model} list="model-options" onChange={(event) => updateActiveAiForm({ model: event.target.value })} placeholder="先获取模型，或手动填写" />
                    <datalist id="model-options">
                      {activeAiForm.modelOptions.map((item) => (
                        <option key={item.id} value={item.id}>{item.name || item.id}</option>
                      ))}
                    </datalist>
                  </label>
                </div>
                <div className="settings-actions">
                  <button type="button" className="neo secondary" disabled={!canFetchModels} onClick={fetchModels}>{activeAiForm.loadingModels ? '获取中...' : '获取模型'}</button>
                  <button type="button" className="neo secondary" disabled={!canFetchModels} onClick={fetchModels}>测试连接</button>
                  <button type="button" className="neo primary" disabled={savingConfig || !activeAiForm.model.trim() || !baseUrl.trim()} onClick={saveAiConfig}>保存 {aiChannel.label}</button>
                </div>
              </>
            )}

            {settingsTab === 'ocr' && (
              <>
                <header className="card-title ocr-title">
                  <h2>📝 OCR 设置</h2>
                  <span>只保留百度 OCR</span>
                </header>
                <div className="form-grid">
                  <label>
                    百度 API Key
                    <input
                      value={baiduApiKey}
                      type="password"
                      autoComplete="off"
                      onChange={(event) => setBaiduApiKey(event.target.value)}
                      placeholder={configInfo?.baidu_ocr?.has_api_key ? `已保存：${configInfo.baidu_ocr.api_key_redacted}` : '百度智能云 API Key'}
                    />
                  </label>
                  <label>
                    百度 Secret Key
                    <input
                      value={baiduSecretKey}
                      type="password"
                      autoComplete="off"
                      onChange={(event) => setBaiduSecretKey(event.target.value)}
                      placeholder={configInfo?.baidu_ocr?.has_secret_key ? `已保存：${configInfo.baidu_ocr.secret_key_redacted}` : '百度智能云 Secret Key'}
                    />
                  </label>
                </div>
                <div className="settings-actions">
                  <button type="button" className="neo primary" disabled={savingConfig} onClick={saveOcrConfig}>保存</button>
                  <span className="hint">截图识别统一走百度 OCR，本地 OCR 已移除。</span>
                </div>
              </>
            )}

            {settingsTab === 'screenshot' && (
              <>
                <header className="card-title shot-title">
                  <h2>◱ 截图设置</h2>
                  <span>截图框位置 · 拖动缩放 · OCR 避让</span>
                </header>
                <div className="setting-grid">
                  <div className="setting-line">
                    <strong>截图框</strong>
                    <span>中间拖动移动，四边/四角缩放，Enter 截图，Esc 取消。</span>
                  </div>
                  <div className="setting-line">
                    <strong>截图避让</strong>
                    <span>截图前隐藏选区并等待 500ms，尽量避免边框文字混入 OCR。</span>
                  </div>
                </div>
                <div className="settings-actions">
                  <button type="button" className="neo secondary" disabled={busy} onClick={toggleScreenshotSelector}>{selectorVisible ? '隐藏截图区域' : '打开截图区域'}</button>
                </div>
              </>
            )}

            {settingsTab === 'history' && (
              <>
                <header className="card-title shot-title">
                  <h2>📒 答题记录</h2>
                  <span>把答过的题写入 history.jsonl · 默认关闭</span>
                </header>
                <div className="setting-grid">
                  <div className="setting-line">
                    <strong>记录开关</strong>
                    <span>
                      开启后，每答完一题会在 <code>{configInfo?.config_file?.replace(/config\.json$/, 'history.jsonl') || 'data/history.jsonl'}</code> 追加一行 JSON，包含题目、诗词命中、AI 答案、各阶段耗时。
                    </span>
                  </div>
                  <div className="setting-line">
                    <strong>当前状态</strong>
                    <span>{configInfo?.history?.enabled ? '✅ 已开启' : '⛔ 已关闭'}</span>
                  </div>
                  <div className="setting-line">
                    <strong>隐私</strong>
                    <span>记录仅写本地文件，不会上传任何服务器。API Key 不会写入。需要分析时可直接用 jq、Python pandas 等工具读取。</span>
                  </div>
                </div>
                <div className="settings-actions">
                  <button
                    type="button"
                    className={configInfo?.history?.enabled ? 'neo secondary' : 'neo primary'}
                    disabled={savingConfig}
                    onClick={() => toggleHistoryEnabled(!configInfo?.history?.enabled)}
                  >
                    {configInfo?.history?.enabled ? '关闭记录' : '开启记录'}
                  </button>
                </div>
              </>
            )}
          </section>
        </main>
      )}
    </div>
  )
}
