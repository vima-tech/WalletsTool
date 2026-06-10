<script setup>
import { useRouter } from 'vue-router'
import { useEcosystemStore } from '@/stores/ecosystem'
import {Notification, Modal, Message} from "@arco-design/web-vue";
import { onMounted, onBeforeUnmount, ref, h, computed } from "vue";
import { useThemeStore } from '@/stores'
import { getVersion } from '@tauri-apps/api/app'
import { WebviewWindow } from '@tauri-apps/api/webviewWindow'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { WINDOW_CONFIG } from '@/utils/windowNames'
import {message} from "@tauri-apps/plugin-dialog";
import { relaunch } from '@tauri-apps/plugin-process'
import packageJson from '@/../package.json'

const router = useRouter()
const ecoStore = useEcosystemStore()
const themeStore = useThemeStore()
let windowCount = ref({})
let windowListObj = ref({})

// Dock 相关状态
const dockRef = ref(null)
const hoveredIndex = ref(-1)
const dockPosition = ref({ x: 0 })

// 窗口透明度
const windowOpacity = ref(1.0)

// 窗口置顶状态
const windowAlwaysOnTop = ref(false)

// 计算dock背景透明度样式
const dockBackgroundStyle = computed(() => {
  // 透明度逻辑：滑块值直接作为透明度
  // 滑块值 0.1-1.0，对应透明度 0.1-1.0（值越大越不透明/越清晰）
  const adjustedOpacity = windowOpacity.value

  // 根据当前主题返回对应的颜色
  const isDark = themeStore.getEffectiveTheme() === 'dark'
  if (isDark) {
    return {
      background: `rgba(22, 27, 34, ${adjustedOpacity})`,
      border: `1px solid rgba(88, 108, 199, ${adjustedOpacity * 0.3})`
    }
  } else {
    return {
      background: `rgba(240, 235, 230, ${adjustedOpacity})`
    }
  }
})

// 计算确认弹窗背景样式 - 确保在白色主题下清晰可见
const confirmModalStyle = computed(() => {
  // 确认弹窗使用纯色背景，不使用透明度
  const isDark = themeStore.getEffectiveTheme() === 'dark'
  if (isDark) {
    return {
      background: `rgba(22, 27, 34, 0.95)`,
      border: `1px solid rgba(88, 108, 199, 0.3)`,
      borderRadius: '16px'
    }
  } else {
    return {
      background: `#ffffff`,
      borderRadius: '16px'
    }
  }
})

// 事件监听器引用，用于清理
let unlistenCloseEvent = null
let unlistenTrayQuitEvent = null
let unlistenOpacityEvent = null
let unlistenAlwaysOnTopEvent = null

// 关闭确认标记位
let closeConfirmed = ref(false)

// 确认弹窗状态跟踪
let isConfirmModalVisible = ref(false)

// 确认弹窗消息
let confirmModalMessage = ref('')

// 数据库管理相关状态
let databaseStatus = ref(null)
let databaseLoading = ref(false)

// 程序版本信息
const runtimeVersion = ref('')
const appVersion = computed(() => runtimeVersion.value || packageJson.version || '0.0.0')

// 更新检查相关状态
let updateChecking = ref(false)
let updateInfo = ref(null)
let hasUpdate = ref(false)
let ignoredVersion = ref('')

const STORAGE_KEYS = {
  lastCheckAt: 'walletstool:update:lastCheckAt',
  ignoreVersion: 'walletstool:update:ignoreVersion',
}

const CHECK_INTERVAL_MS = 12 * 60 * 60 * 1000

// 主题切换相关状态 - 使用computed从themeStore获取
const isDarkTheme = computed(() => themeStore.getEffectiveTheme() === 'dark')

onMounted(async () => {
  try {
    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
    if (isTauri) runtimeVersion.value = await getVersion()
  } catch (error) {
    console.error('Failed to get app version:', error)
  }

  // 自动根据dock items数量设置主窗口大小
  try {
    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
    if (isTauri) {
      // 只传递功能items数量，后端会自动加上设置和退出按钮
      const itemCount = dockItems.length
      await invoke('set_main_window_size_for_dock', { itemCount })
    }
  } catch (error) {
    console.error('Failed to set main window size:', error)
  }

  // 应用保存的窗口透明度设置
  try {
    const savedOpacity = localStorage.getItem('mainWindowOpacity')
    if (savedOpacity) {
      windowOpacity.value = parseFloat(savedOpacity)
    }
  } catch (error) {
    console.error('Failed to apply window opacity:', error)
  }

  // 监听主窗口关闭请求事件
  try {
    // 检查是否在Tauri环境中
    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__;
    if (isTauri) {
      unlistenCloseEvent = await listen('main-window-close-requested', async () => {
        await handleMainWindowCloseRequest()
      })

      // 监听托盘退出请求事件
      unlistenTrayQuitEvent = await listen('tray-quit-requested', async () => {
        await handleMainWindowCloseRequest()
      })

      // 监听透明度变化事件（从设置窗口发送）
      unlistenOpacityEvent = await listen('window-opacity-changed', (event) => {
        const { opacity } = event.payload
        windowOpacity.value = opacity
        // 保存到localStorage以便下次启动使用
        localStorage.setItem('mainWindowOpacity', opacity.toString())
      })

      // 监听置顶状态变化事件（从设置窗口发送）
      unlistenAlwaysOnTopEvent = await listen('window-always-on-top-changed', async (event) => {
        console.log('[AlwaysOnTop] 收到置顶状态变化事件:', event.payload)
        const { alwaysOnTop } = event.payload
        windowAlwaysOnTop.value = alwaysOnTop
        localStorage.setItem('mainWindowAlwaysOnTop', alwaysOnTop.toString())
        
        try {
          console.log('[AlwaysOnTop] 调用后端命令设置主窗口置顶:', alwaysOnTop)
          await invoke('set_main_window_always_on_top', { alwaysOnTop })
          console.log('[AlwaysOnTop] 后端命令调用成功')
        } catch (err) {
          console.error('[AlwaysOnTop] 设置置顶失败:', err)
        }
      })
    }
  } catch (error) {
    console.error('Failed to listen for close event:', error)
  }

  // 加载保存的置顶设置并应用到主窗口
  try {
    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
    if (isTauri) {
      const savedAlwaysOnTop = localStorage.getItem('mainWindowAlwaysOnTop')
      if (savedAlwaysOnTop === 'true') {
        windowAlwaysOnTop.value = true
        await invoke('set_main_window_always_on_top', { alwaysOnTop: true })
      }
    }
  } catch (error) {
    console.error('Failed to apply always on top setting:', error)
  }

  // 注意：主窗口的显示由 SplashScreen 组件控制，这里不需要再次调用 show()
  // 启动窗口会在加载完成后自动显示主窗口并关闭自己

  // 启动时检查更新（仅显示badge，不弹窗）
  setTimeout(() => {
    checkForUpdateSilent()
  }, 2000)
})

// 组件卸载时清理事件监听器
onBeforeUnmount(() => {
  if (unlistenCloseEvent) {
    unlistenCloseEvent()
  }
  if (unlistenTrayQuitEvent) {
    unlistenTrayQuitEvent()
  }
  if (unlistenOpacityEvent) {
    unlistenOpacityEvent()
  }
  if (unlistenAlwaysOnTopEvent) {
    unlistenAlwaysOnTopEvent()
  }
})

// Dock 功能列表
const dockItems = [
  {
    id: 'wallet-manager',
    title: '钱包',
    desc: '批量管理私钥/助记词/地址',
    icon: 'wallet',
    color: '#586cc7'
  },
  {
    id: 'transfer',
    title: '转账',
    desc: 'EVM/Solana 批量转账',
    icon: 'transfer',
    color: '#52c41a'
  },
  {
    id: 'balance',
    title: '余额',
    desc: '多链资产查询导出',
    icon: 'balance',
    color: '#faad14'
  },
  {
    id: 'airdrop-browser',
    title: '浏览器',
    desc: 'Playwright 自动化',
    icon: 'browser',
    color: '#13c2c2'
  },
  {
    id: 'distribution',
    title: '分发',
    desc: '单钱包快速分发',
    icon: 'rocket',
    isBuilding: true,
    color: '#eb2f96'
  },
  {
    id: 'monitor',
    title: '监控',
    desc: '地址分析监控',
    icon: 'monitor',
    isBuilding: true,
    color: '#722ed1'
  }
]

// Dock 鼠标交互
const handleDockMouseMove = (e) => {
  if (!dockRef.value) return
  const rect = dockRef.value.getBoundingClientRect()
  dockPosition.value.x = e.clientX - rect.left
}

const handleDockMouseLeave = () => {
  hoveredIndex.value = -1
}

const getDockItemStyle = (index) => {
  if (hoveredIndex.value === -1) return {}

  const distance = Math.abs(index - hoveredIndex.value)
  if (distance > 2) return {}

  const scale = distance === 0 ? 1.15 : distance === 1 ? 1.08 : 1.02

  return {
    transform: `scale(${scale})`,
    transition: 'transform 0.25s cubic-bezier(0.34, 1.56, 0.64, 1)',
    zIndex: distance === 0 ? 10 : distance === 1 ? 5 : 2
  }
}

const handleItemHover = (index) => {
  hoveredIndex.value = index
}

// 窗口拖拽
const handleDragStart = async (e) => {
  const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
  if (!isTauri) return

  // 只响应左键
  if (e.button !== 0) return

  // 检查点击目标是否是图标点击区域或文字标签
  const target = e.target
  const isIconHitbox = target.closest('.dock-icon-hitbox')
  const isDockLabel = target.closest('.dock-label')
  const isDockItem = target.closest('.dock-item')

  if (isIconHitbox || isDockLabel || isDockItem) return

  try {
    const currentWindow = getCurrentWindow()
    await currentWindow.startDragging()
  } catch (error) {
    console.error('拖拽失败:', error)
  }
}

const handleItemClick = (item) => {
  if (item.isBuilding) {
    Message.warning('功能建设中，敬请期待')
    return
  }
  goPage(item.id)
}

const adjustColor = (color, amount) => {
  const hex = color.replace('#', '')
  const num = parseInt(hex, 16)
  const r = Math.min(255, Math.max(0, (num >> 16) + amount))
  const g = Math.min(255, Math.max(0, ((num >> 8) & 0x00FF) + amount))
  const b = Math.min(255, Math.max(0, (num & 0x0000FF) + amount))
  return `#${((r << 16) | (g << 8) | b).toString(16).padStart(6, '0')}`
}

// 跳转逻辑
function goPage(pageName) {

  const targetModule = dockItems.find(item => item.id === pageName);
  if (targetModule?.isBuilding) {
    Message.warning('功能建设中，敬请期待')
    return
  }

  // 检查是否在Tauri环境中
  const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__;
  if (!isTauri) {
    // 浏览器环境：跳转到入口页
    router.push(`/entry?target=${pageName}`)
    return
  }

  // 钱包管理页面单例模式：如果已存在窗口，则将其置顶显示
  if (pageName === 'wallet-manager') {
    const existingWindows = windowListObj.value['wallet-manager']
    if (existingWindows && existingWindows.size > 0) {
      // 获取第一个已存在的窗口
      const firstEntry = existingWindows.entries().next().value
      if (firstEntry) {
        const [existingLabel, existingWebview] = firstEntry
        // 检查窗口是否还实际存在
        const isValid = existingWebview && typeof existingWebview.setFocus === 'function'
        if (isValid) {
          // 尝试将已存在的窗口置顶显示
          existingWebview.setFocus().then(() => {
            return existingWebview.show()
          }).then(() => {
            console.log('[Home] wallet-manager window already exists, bringing to front:', existingLabel)
          }).catch(err => {
            console.warn('[Home] wallet-manager window no longer valid, creating new one:', err)
            // 从列表中移除无效的窗口
            windowListObj.value['wallet-manager'].delete(existingLabel)
            // 继续创建新窗口
            createNewWindow(pageName)
          })
          return
        } else {
          // 窗口对象无效，清理并创建新窗口
          console.warn('[Home] wallet-manager window object invalid, cleaning up')
          windowListObj.value['wallet-manager'].delete(existingLabel)
        }
      }
    }
  }

  createNewWindow(pageName)
}

// 创建新窗口
function createNewWindow(pageName) {
  try {
    // 正确实现多窗口
    const count = windowCount.value[pageName] ?? 0
    windowCount.value[pageName] = count + 1
    const newCount = windowCount.value[pageName]
    if (!windowListObj.value[pageName]) {
      windowListObj.value[pageName] = new Map()
    }
    const windowLabel = WINDOW_CONFIG.generateLabel(pageName, newCount)

    // 直接路由到目标页面，跳过 EcosystemEntry，利用 localStorage 中保存的生态系统选择
    const eco = ecoStore.currentEco || 'eth'
    const windowUrl = pageName === 'airdrop-browser'
      ? `/#/airdrop/browser?count=${newCount}`
      : pageName === 'wallet-manager'
        ? `/#/wallet-manager?count=${newCount}`
        : pageName === 'monitor'
          ? `/#/eth/monitor?count=${newCount}`
          : `/#/${eco}/${pageName}?count=${newCount}`

    // 生成窗口标题：统一格式 "WalletsTool - {图标} {功能名} [{序号}]"
    const moduleIcons = { transfer: '💸', balance: '💰', monitor: '👁️', 'airdrop-browser': '🤖', 'wallet-manager': '🔐' }
    const moduleNames = { transfer: '批量转账', balance: '余额查询', monitor: '链上监控', 'airdrop-browser': '浏览器自动化', 'wallet-manager': '钱包管理' }
    const title = newCount > 1
      ? `WalletsTool - ${moduleIcons[pageName] || ''} ${moduleNames[pageName] || pageName} [${newCount}]`
      : `WalletsTool - ${moduleIcons[pageName] || ''} ${moduleNames[pageName] || pageName}`

    let isShown = false
    let fallbackShowTimer = null

    const showWindowOnce = () => {
      if (isShown) return
      isShown = true
      if (fallbackShowTimer) {
        clearTimeout(fallbackShowTimer)
        fallbackShowTimer = null
      }
      webview.show()
    }

    const webview = new WebviewWindow(windowLabel, {
      url: windowUrl,
      width: 1350,
      height: 900,
      title: title,
      resizable: true,
      center: true,
      decorations: false,
      backgroundColor: document.documentElement.getAttribute('data-theme') === 'light' ? '#FFFFFF' : '#2A2A2B',
      visible: false,
    })

    windowListObj.value[pageName].set(windowLabel, webview)
    fallbackShowTimer = setTimeout(showWindowOnce, 1500)

    webview.once('tauri://close-requested', function (event) {
      // 在 Tauri 2.x 中，需要手动关闭窗口
      webview.close()
    })

    webview.once('tauri://destroyed', function (event) {
      console.log('[Home] window destroyed:', windowLabel, event)
      if (fallbackShowTimer) {
        clearTimeout(fallbackShowTimer)
        fallbackShowTimer = null
      }
      // 使用创建时的 windowLabel 而不是 event.windowLabel
      windowListObj.value[pageName].delete(windowLabel)
      if (windowListObj.value[pageName].size === 0) {
        windowCount.value[pageName] = 0
      }
    })

    webview.once('tauri://error', function (e) {
      console.error('Window creation error:', e)
    })

    webview.listen('page-loaded', showWindowOnce)

  } catch (error) {
    console.error('Error in createNewWindow:', error)
  }
}

// 切换调试模式


// 切换主题
function toggleTheme() {
  themeStore.toggleTheme()
  // if (isDarkTheme.value) {
  //   Notification.success({ content: '已切换到暗黑主题', position: 'topLeft' })
  // } else {
  //   Notification.success({ content: '已切换到明亮主题', position: 'topLeft' })
  // }
}

// 检查数据库状态
async function checkDatabaseStatus() {
  try {
    databaseLoading.value = true
    let status
    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
    if (isTauri) {
      status = await invoke('check_database_schema')
    } else {
      // 浏览器环境下模拟正常状态
      status = {
        db_exists: true,
        chains_table_exists: true,
        tokens_table_exists: true,
        rpc_table_exists: true,
        abi_column_exists: true,
        contract_type_column_exists: true,
        needs_migration: false
      }
    }

    // 将状态对象转换为友好的中文描述
    let statusText = ''
    let notificationType = 'success'

    if (typeof status === 'object' && status !== null) {
      // 根据新的检查逻辑生成状态文本
      if (!status.db_exists) {
        statusText = '❌ 数据库文件不存在'
        notificationType = 'error'
      } else if (!(status.chains_table_exists && status.tokens_table_exists && status.rpc_table_exists)) {
        const missingTables = []
        if (!status.chains_table_exists) missingTables.push('链表(chains)')
        if (!status.tokens_table_exists) missingTables.push('代币表(tokens)')
        if (!status.rpc_table_exists) missingTables.push('RPC表(rpc_providers)')
        statusText = `❌ 数据库缺少必要表：\n${missingTables.join('\n')}`
        notificationType = 'error'
      } else if (status.needs_migration) {
        statusText = '⚠️ 数据库需要迁移更新'
        notificationType = 'warning'
      } else {
        statusText = '✅ 数据库结构完整，运行正常'
      }
    } else {
      statusText = typeof status === 'string' ? status : JSON.stringify(status)
    }

    databaseStatus.value = statusText

    if (notificationType === 'success') {
      Notification.success({ 
        title: '数据库状态检查完成',
        content: statusText
      , position: 'topLeft' })
    } else if (notificationType === 'warning') {
      Notification.warning({ 
        title: '数据库状态检查完成',
        content: statusText
      , position: 'topLeft' })
    } else {
      Notification.error({ 
        title: '数据库状态检查完成',
        content: statusText
      , position: 'topLeft' })
    }

    // 数据库状态检查完成
  } catch (error) {
    console.error('检查数据库状态失败:', error)
    const errorText = typeof error === 'string' ? error : error.message || '未知错误'
    databaseStatus.value = '检查失败: ' + errorText
    Notification.error({ 
      title: '检查数据库状态失败',
      content: errorText
    , position: 'topLeft' })
  } finally {
    databaseLoading.value = false
  }
}

// 恢复出厂设置
async function reloadDatabase() {
  try {
    databaseLoading.value = true
    let result
    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
    if (isTauri) {
      result = await invoke('reload_database')
    } else {
      // 浏览器环境下模拟成功
      result = '恢复出厂设置成功'
    }

    // 确保result是字符串格式
    const resultText = typeof result === 'string' ? result : JSON.stringify(result)

    Notification.success({ 
      title: '恢复出厂设置完成',
      content: resultText
    , position: 'topLeft' })

    // 重新检查数据库状态
    setTimeout(async () => {
      await checkDatabaseStatus()
    }, 500)

  } catch (error) {
    console.error('恢复出厂设置失败:', error)
    const errorText = typeof error === 'string' ? error : error.message || '未知错误'
    Notification.error({ 
      title: '恢复出厂设置失败',
      content: errorText
    , position: 'topLeft' })
  } finally {
    databaseLoading.value = false
  }
}

// 刷新页面数据
async function refreshPageData() {
  try {
    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
    if (isTauri) {
      // 在Home页面，主要是刷新一些基础数据
      // 可以根据需要添加更多刷新逻辑
    }

    // 重置数据库状态
    databaseStatus.value = null

    Notification.success({ 
      title: '页面数据已刷新',
      content: '所有状态已重置'
    , position: 'topLeft' })

    // 自动重新检查数据库状态
    setTimeout(async () => {
      await checkDatabaseStatus()
    }, 300)

  } catch (error) {
    console.error('刷新页面数据失败:', error)
    const errorText = typeof error === 'string' ? error : error.message || '未知错误'
    Notification.error({ 
      title: '刷新页面数据失败',
      content: errorText
    , position: 'topLeft' })
  }
}

// 导出数据库数据到init.sql
async function exportDatabaseToInitSql() {
  try {
    databaseLoading.value = true
    let result
    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
    if (isTauri) {
      result = await invoke('export_database_to_init_sql')
    } else {
      // 浏览器环境下模拟成功
      result = '数据库导出成功（浏览器环境模拟）'
    }

    // 确保result是字符串格式
    const resultText = typeof result === 'string' ? result : JSON.stringify(result)

    Notification.success({
      title: '导出 public.db 成功',
      content: resultText
    , position: 'topLeft' })

    // 数据库导出完成

  } catch (error) {
    console.error('导出数据库失败:', error)
    const errorText = typeof error === 'string' ? error : error.message || '未知错误'
    Notification.error({
      title: '导出数据库失败',
      content: errorText
    , position: 'topLeft' })
  } finally {
    databaseLoading.value = false
  }
}

// 检查是否应该检查更新（根据时间间隔）
function shouldCheckUpdate() {
  const lastCheckAtRaw = localStorage.getItem(STORAGE_KEYS.lastCheckAt)
  const lastCheckAt = lastCheckAtRaw ? Number(lastCheckAtRaw) : 0
  if (!Number.isFinite(lastCheckAt)) return true
  return Date.now() - lastCheckAt >= CHECK_INTERVAL_MS
}

// 设置最后检查时间
function setLastCheckNow() {
  localStorage.setItem(STORAGE_KEYS.lastCheckAt, String(Date.now()))
}

// 获取被忽略的版本
function getIgnoredVersion() {
  return localStorage.getItem(STORAGE_KEYS.ignoreVersion) || ''
}

// 忽略当前版本
function ignoreCurrentVersion() {
  if (updateInfo.value?.latest_version) {
    localStorage.setItem(STORAGE_KEYS.ignoreVersion, updateInfo.value.latest_version)
    hasUpdate.value = false
  }
}

// 静默检查更新（仅设置badge，不弹窗）
async function checkForUpdateSilent() {
  try {
    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
    if (!isTauri) return

    // 检查时间间隔
    if (!shouldCheckUpdate()) {
      // 检查是否有已保存的更新但未忽略
      const savedIgnored = getIgnoredVersion()
      // 如果有已忽略的，保持hasUpdate为false
      // 如果没有，可能需要重新检查
      return
    }

    const result = await invoke('check_update', {
      currentVersion: appVersion.value
    })

    setLastCheckNow()

    if (result.has_update) {
      const ignored = getIgnoredVersion()
      // 如果当前版本已被忽略，不显示badge
      if (ignored && ignored === result.latest_version) {
        hasUpdate.value = false
      } else {
        hasUpdate.value = true
        updateInfo.value = result
      }
    } else {
      hasUpdate.value = false
    }

  } catch (error) {
    console.error('静默检查更新失败:', error)
    // 静默失败，不显示任何提示
  }
}

// 手动检查更新（显示结果）
async function checkForUpdate() {
  try {
    updateChecking.value = true
    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__

    if (!isTauri) {
      Notification.warning({
        title: '检查更新',
        content: '浏览器环境下无法检查更新',
        position: 'topLeft'
      })
      return
    }

    const result = await invoke('check_update', {
      currentVersion: appVersion.value
    })

    setLastCheckNow()
    updateInfo.value = result

    if (result.has_update) {
      hasUpdate.value = true
      // 显示更新对话框
      Modal.confirm({
        title: '发现新版本',
        content: () => h('div', {
          style: 'max-height: 300px; overflow-y: auto;'
        }, [
          h('div', { style: 'margin-bottom: 12px;' }, [
            h('span', { style: 'color: #666;' }, '当前版本: '),
            h('span', { style: 'font-weight: 600; color: #586cc7;' }, result.current_version)
          ]),
          h('div', { style: 'margin-bottom: 12px;' }, [
            h('span', { style: 'color: #666;' }, '最新版本: '),
            h('span', { style: 'font-weight: 600; color: #52c41a;' }, result.latest_version)
          ]),
          result.published_at ? h('div', { style: 'margin-bottom: 12px; font-size: 12px; color: #999;' },
            `发布时间: ${result.published_at}`) : null,
          h('div', { style: 'margin-top: 16px;' }, [
            h('div', { style: 'font-weight: 600; margin-bottom: 8px;' }, '更新内容:'),
            h('div', {
              style: 'background: rgba(88, 108, 199, 0.05); padding: 12px; border-radius: 8px; font-size: 13px; line-height: 1.6; white-space: pre-wrap;'
            }, result.release_notes || '暂无更新说明')
          ])
        ]),
        okText: '下载并安装',
        cancelText: '稍后提醒',
        width: Math.min(420, Math.max(320, Math.floor(window.innerWidth * 0.92))),
        onOk: async () => {
          await downloadAndInstallUpdate()
        },
        onCancel: () => {
          // 用户选择稍后提醒，不忽略版本
        }
      })
    } else {
      hasUpdate.value = false
      Notification.success({
        title: '检查更新完成',
        content: `当前版本 v${result.current_version} 已是最新版本`,
        position: 'topLeft'
      })
    }

  } catch (error) {
    console.error('检查更新失败:', error)
    const errorText = typeof error === 'string' ? error : error.message || '未知错误'
    Notification.error({
      title: '检查更新失败',
      content: errorText,
      position: 'topLeft'
    })
  } finally {
    updateChecking.value = false
  }
}

// 下载并安装更新
async function downloadAndInstallUpdate() {
  try {
    updateChecking.value = true

    Notification.info({
      title: '正在下载更新',
      content: '请稍候，下载完成后将自动安装并重启',
      position: 'topLeft',
      duration: 0
    })

    const result = await invoke('download_and_install_update')

    Notification.success({
      title: '更新完成',
      content: result,
      position: 'topLeft'
    })

    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
    if (isTauri) await relaunch()
  } catch (error) {
    console.error('下载更新失败:', error)
    const errorText = typeof error === 'string' ? error : error.message || '未知错误'
    Notification.error({
      title: '下载更新失败',
      content: errorText,
      position: 'topLeft'
    })
  } finally {
    updateChecking.value = false
  }
}

// 标题栏控制方法
async function minimizeWindow() {
  try {
    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__;
    if (isTauri) {
      const currentWindow = getCurrentWindow()
      await currentWindow.minimize()
    }
  } catch (error) {
    console.error('Error minimizing window:', error)
  }
}

// 最小化到托盘
async function minimizeToTray() {
  try {
    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__;
    if (isTauri) {
      const currentWindow = getCurrentWindow()
      await currentWindow.hide()
    }
  } catch (error) {
    console.error('Error minimizing to tray:', error)
  }
}

async function closeWindow() {
  try {
    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__;
    if (isTauri) {
      // 调用确认关闭函数而不是直接关闭窗口
      await handleMainWindowCloseRequest()
    }
  } catch (error) {
    console.error('Error closing window:', error)
  }
}

// 打开设置窗口
async function openSettings() {
  try {
    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
    if (!isTauri) {
      Notification.warning({
        title: '设置',
        content: '浏览器环境下无法打开设置窗口',
        position: 'topLeft'
      })
      return
    }

    // 检查设置窗口是否已存在，如果存在则聚焦显示
    try {
      const existingWindow = await WebviewWindow.getByLabel('settings')
      if (existingWindow) {
        console.log('[Settings] 窗口已存在，聚焦显示')
        await existingWindow.setFocus()
        await existingWindow.show()
        return
      }
    } catch (e) {
      console.log('[Settings] 检查已存在窗口时出错（可忽略）:', e)
    }

    // 创建设置窗口
    const settingsWindow = new WebviewWindow('settings', {
      url: '/#/settings',
      title: '设置',
      width: 400,
      height: 500,
      resizable: false,
      decorations: false,
      center: true,
      alwaysOnTop: true,
      transparent: true
    })

    settingsWindow.once('tauri://created', () => {
      console.log('设置窗口已创建')
    })

    settingsWindow.once('tauri://error', (e) => {
      console.error('创建设置窗口失败:', e)
      Notification.error({
        title: '错误',
        content: '创建设置窗口失败'
      , position: 'topLeft' })
    })
  } catch (error) {
    console.error('打开设置窗口失败:', error)
    Notification.error({
      title: '错误',
      content: '打开设置窗口失败: ' + error.message
    , position: 'topLeft' })
  }
}

// 清除所有代理配置缓存
async function clearAllProxyConfigs() {
  const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__;
  
  // 清除前端localStorage
  const keysToRemove = [];
  for (let i = 0; i < localStorage.length; i++) {
    const key = localStorage.key(i);
    if (key && (key.startsWith('proxy_config_') || key.startsWith('proxy_window_id_'))) {
      keysToRemove.push(key);
    }
  }
  keysToRemove.forEach(key => {
    localStorage.removeItem(key);
    console.log(`已清除缓存: ${key}`);
  });
  console.log(`已清除 ${keysToRemove.length} 个代理配置缓存`);
  
  // 清除后端文件缓存和内存缓存
  if (isTauri) {
    try {
      const currentWindow = getCurrentWindow();
      await invoke('clear_proxy_config_for_window', { windowId: currentWindow.label });
      console.log(`已清除窗口 ${currentWindow.label} 的后端代理配置`);
    } catch (error) {
      console.error('清除后端代理配置失败:', error);
    }
  }
}

// 关闭确认弹窗
function hideConfirmModal() {
  isConfirmModalVisible.value = false
  closeConfirmed.value = false
}

// 确认关闭操作
async function confirmClose() {
  try {
    closeConfirmed.value = true
    isConfirmModalVisible.value = false

    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
    if (!isTauri) return

    const childWindows = await invoke('get_all_child_windows', {
      mainWindowLabel: 'main'
    })

    await clearAllProxyConfigs()

    if (childWindows && childWindows.length > 0) {
      await invoke('close_all_child_windows', {
        mainWindowLabel: 'main'
      })
      await new Promise(resolveTimeout => setTimeout(resolveTimeout, 500))
    }

    await invoke('force_close_main_window')
  } catch (error) {
    console.error('关闭窗口时发生错误:', error)
    closeConfirmed.value = false
    isConfirmModalVisible.value = false
    Notification.error({
      title: '错误',
      content: '关闭窗口时发生错误，请重试'
    , position: 'topLeft' })
  }
}

// 处理主窗口关闭请求
async function handleMainWindowCloseRequest() {
  try {
    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__;
    if (!isTauri) {
      return true
    }

    if (isConfirmModalVisible.value) {
      return false
    }

    if (closeConfirmed.value) {
      return true
    }

    const childWindows = await invoke('get_all_child_windows', {
      mainWindowLabel: 'main'
    })

    if (childWindows && childWindows.length > 0) {
      confirmModalMessage.value = `当前还有 ${childWindows.length} 个子窗口正在运行\n关闭主窗口将关闭所有窗口`
    } else {
      confirmModalMessage.value = '确定要关闭应用程序吗？'
    }

    isConfirmModalVisible.value = true

  } catch (error) {
    console.error('处理窗口关闭请求时发生错误:', error)
    confirmModalMessage.value = '确定要关闭应用程序吗？'
    isConfirmModalVisible.value = true
  }
}
</script>

<template>
  <div class="container home" :class="{ 'light-theme': !isDarkTheme }">
    <!-- 顶部拖拽区域 -->
    <div class="drag-region"></div>
    <!-- 纯 Dock 样式 -->
    <div class="main-content">
      <div
        class="dock"
        ref="dockRef"
        :style="dockBackgroundStyle"
        @mousemove="handleDockMouseMove"
        @mousedown="handleDragStart"
      >
        <div class="dock-bg"></div>
        
        <div
          v-for="(item, index) in dockItems"
          :key="item.id"
          class="dock-item"
          :class="{
            'disabled': item.isBuilding
          }"
          :style="getDockItemStyle(index)"
          @mouseenter="handleItemHover(index)"
          @mouseleave="handleDockMouseLeave"
          @click="handleItemClick(item)"
        >
          <div class="dock-icon" :style="{ background: `linear-gradient(135deg, ${item.color}, ${adjustColor(item.color, -30)})` }">
            <div class="dock-icon-hitbox">
              <svg v-if="item.icon === 'wallet'" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <rect x="2" y="5" width="20" height="14" rx="2"/>
                <path d="M16 12h.01"/>
                <path d="M2 10h20"/>
                <circle cx="16" cy="12" r="1" fill="currentColor"/>
              </svg>
              <svg v-else-if="item.icon === 'transfer'" class="custom-icon" viewBox="0 0 1024 1024">
                <path d="M1.052856 278.365165c0-110.542612 90.062799-200.613475 200.82311-200.82311h129.361465l-33.864258-33.864258a25.478822 25.478822 0 0 1 0-36.146064 25.478822 25.478822 0 0 1 36.162189 0l77.524962 77.533025c4.813562 4.805499 7.522703 11.489659 7.522702 18.181881 0 6.893795-2.709141 13.368319-7.522702 18.181881l-77.524962 77.524961a25.454634 25.454634 0 0 1-18.181881 7.522703c-6.692222 0-13.166746-2.499505-18.181881-7.522703a25.478822 25.478822 0 0 1 0-36.154126l33.856195-33.848132H201.658268c-82.338524 0-149.413942 67.083482-149.413943 149.413942v91.538314c0 14.206862-11.288086 25.696521-25.494948 25.696521A25.688458 25.688458 0 0 1 1.052856 369.903479V278.365165z m475.421925 246.386663c13.997226 0 25.494948 11.497722 25.704584 25.494948v448.048317A25.688458 25.688458 0 0 1 476.474781 1023.999677H28.628037a25.688458 25.688458 0 0 1-25.704584-25.704584V550.456412a25.688458 25.688458 0 0 1 25.704584-25.704584h447.838681z m-25.704584 447.838681h0.209636V575.95136H54.340684v396.639149h396.429513z m546.472363-344.181801a25.688458 25.688458 0 0 1 25.704584 25.696521v91.74795c0 110.542612-90.062799 200.613475-200.613475 200.613474H692.972205l33.864258 33.864258a25.478822 25.478822 0 0 1 0 36.146064 25.454634 25.454634 0 0 1-18.189944 7.522702c-6.692222 0-13.166746-2.507568-18.181881-7.522702l-77.524962-77.533025a25.607829 25.607829 0 0 1-7.522702-18.181881c0-6.893795 2.709141-13.368319 7.522702-18.181881l77.524962-77.524961a25.478822 25.478822 0 0 1 36.154126 0 25.478822 25.478822 0 0 1 0 36.154126l-33.856195 33.848132h129.361465c82.338524 0 149.413942-67.083482 149.413942-149.413942V654.105229a25.688458 25.688458 0 0 1 25.704584-25.696521zM995.371963 0.218666c14.214925 0 25.494948 11.489659 25.704584 25.704584v447.838682a25.688458 25.688458 0 0 1-25.704584 25.704584H547.525219a25.688458 25.688458 0 0 1-25.704584-25.704584V2.92325a25.688458 25.688458 0 0 1 25.704584-25.704584h447.838682z m-25.704584 448.048318h0.209636V51.218666H573.229803v397.048318h396.437576z" fill="currentColor"/>
              </svg>
              <svg v-else-if="item.icon === 'balance'" class="custom-icon" viewBox="0 0 1024 1024">
                <path d="M67.723636 766.603636a46.545455 46.545455 0 0 1 80.709819-46.312727A418.909091 418.909091 0 1 0 93.090909 512a46.545455 46.545455 0 0 1-93.044364 0C0 229.236364 229.236364 0 512 0s512 229.236364 512 512-229.236364 512-512 512a511.767273 511.767273 0 0 1-444.276364-257.396364z" fill="currentColor"/>
                <path d="M649.448727 372.363636a46.545455 46.545455 0 1 0 0-93.090909h-229.934545a93.090909 93.090909 0 0 0-93.090909 93.090909v69.818182a93.090909 93.090909 0 0 0 93.090909 93.090909H605.090909V605.090909H372.363636a46.545455 46.545455 0 0 0 0 93.090909h232.727273a93.090909 93.090909 0 0 0 93.090909-93.090909v-69.818182a93.090909 93.090909 0 0 0-93.090909-93.090909H419.560727V372.363636h229.934546z" fill="currentColor"/>
                <path d="M558.545455 744.587636a46.545455 46.545455 0 1 1-93.09091 0v-92.811636a46.545455 46.545455 0 1 1 93.09091 0v92.811636zM558.545455 325.818182a46.545455 46.545455 0 0 1-93.09091 0V232.727273a46.545455 46.545455 0 0 1 93.09091 0v93.090909z" fill="currentColor"/>
              </svg>
              <svg v-else-if="item.icon === 'rocket'" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09z"/>
                <path d="M12 15l-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2z"/>
                <path d="M9 12H4s.55-3.03 2-4c1.62-1.08 5 0 5 0"/>
                <path d="M12 15v5s3.03-.55 4-2c1.08-1.62 0-5 0-5"/>
              </svg>
              <svg v-else-if="item.icon === 'monitor'" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M12 2L2 7l10 5 10-5-10-5z"/>
                <path d="M2 17l10 5 10-5"/>
                <path d="M2 12l10 5 10-5"/>
              </svg>
              <svg v-else-if="item.icon === 'browser'" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <rect x="3" y="3" width="18" height="18" rx="2"/>
                <path d="M3 9h18"/>
                <path d="M9 21V9"/>
                <circle cx="6" cy="6" r="0.5" fill="currentColor"/>
                <circle cx="9" cy="6" r="0.5" fill="currentColor"/>
                <circle cx="12" cy="6" r="0.5" fill="currentColor"/>
              </svg>
            </div>
          </div>

          <span v-if="item.isBuilding" class="dock-badge building"></span>

          <div class="dock-label">{{ item.title }}</div>
        </div>

        <!-- 设置入口 - 9点样式 -->
        <div class="dock-divider"></div>
        <div
          class="dock-item settings-item"
          :style="getDockItemStyle(dockItems.length)"
          @mouseenter="handleItemHover(dockItems.length)"
          @mouseleave="handleDockMouseLeave"
          @click="openSettings"
        >
          <div class="dock-icon settings-icon">
            <div class="dock-icon-hitbox">
              <div class="grid-dots">
                <span></span><span></span><span></span>
                <span></span><span></span><span></span>
                <span></span><span></span><span></span>
              </div>
            </div>
          </div>
          <!-- 更新角标 NEW -->
          <span v-if="hasUpdate" class="dock-corner-badge">NEW</span>
          <div class="dock-label">设置</div>
        </div>

        <!-- 关闭按钮 -->
        <div class="dock-divider"></div>
        <div
          class="dock-item close-item"
          :style="getDockItemStyle(dockItems.length + 1)"
          @mouseenter="handleItemHover(dockItems.length + 1)"
          @mouseleave="handleDockMouseLeave"
          @click="closeWindow"
        >
          <div class="dock-icon close-icon">
            <div class="dock-icon-hitbox">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M18 6L6 18"/>
                <path d="M6 6l12 12"/>
              </svg>
            </div>
          </div>
          <div class="dock-label">退出</div>
        </div>
      </div>
    </div>

    <!-- 自定义确认关闭弹窗 - Dock风格 -->
    <Teleport to="body">
      <Transition name="confirm-fade">
        <div v-if="isConfirmModalVisible" class="confirm-overlay" @click.self="hideConfirmModal">
          <div class="confirm-modal" :class="{ 'light-theme': !isDarkTheme }" :style="confirmModalStyle">
            <div class="confirm-icon">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <circle cx="12" cy="12" r="10"/>
                <path d="M12 8v4"/>
                <path d="M12 16h.01"/>
              </svg>
            </div>
            <div class="confirm-content">
              <div class="confirm-title">确认关闭</div>
              <div class="confirm-message">{{ confirmModalMessage }}</div>
            </div>
            <div class="confirm-actions">
              <button class="confirm-btn cancel" @click="hideConfirmModal">取消</button>
              <button class="confirm-btn danger" @click="confirmClose">确定</button>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<style scoped>
/* 主容器 - 透明背景 */
.container {
  position: relative;
  width: 100%;
  height: 100vh;
  background: transparent;
  overflow: hidden;
  box-sizing: border-box;
  display: flex;
  justify-content: center;
  align-items: center;
  user-select: none;
  -webkit-user-select: none;
  /* 设置较大的圆角 */
  border-radius: 18px;
}

/* 顶部拖拽区域 */
.drag-region {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  height: 12px;
  -webkit-app-region: drag;
  z-index: 9999;
  cursor: move;
}

/* 主内容区域 */
.main-content {
  display: flex;
  justify-content: center;
  align-items: center;
  position: relative;
  z-index: 1;
  width: 100%;
  height: 100%;
  /* 减小padding让dock贴近窗口边缘，避免padding区域出现透明痕迹 */
  padding: 8px;
}

/* Dock栏 - 可拖拽 */
.dock {
  cursor: grab;
}

.dock:active {
  cursor: grabbing;
}

/* Dock 栏 - 玻璃态背景（灰褐色） */
.dock {
  display: flex;
  align-items: flex-end;
  gap: 6px;
  padding: 20px 16px 12px;
  position: relative;
  /* 设置较大的圆角 */
  border-radius: 18px;
  /* 背景色由动态样式控制，支持透明度调节 */
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
}

.dock-bg {
  display: none;
}

/* Dock 分隔线 */
.dock-divider {
  width: 1px;
  height: 50px;
  background: rgba(255, 255, 255, 0.15);
  margin: 0 6px;
  align-self: center;
}

.dock-item {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  cursor: pointer;
  transform-origin: center bottom;
  transition: transform 0.25s cubic-bezier(0.34, 1.56, 0.64, 1);
  z-index: 1;
  padding: 8px 4px 4px;
}

.dock-item.disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.dock-icon {
  width: 40px;
  height: 40px;
  border-radius: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: transform 0.25s cubic-bezier(0.34, 1.56, 0.64, 1);
}

.dock-icon-hitbox {
  width: 40px;
  height: 40px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.dock-icon svg {
  width: 20px;
  height: 20px;
  color: white;
  filter: drop-shadow(0 2px 4px rgba(0, 0, 0, 0.2));
  pointer-events: none;
}

.dock-item:hover {
  z-index: 10;
}

.dock-item:hover .dock-icon {
  transform: scale(1.15);
}

.dock-item:active .dock-icon {
  transform: scale(0.95);
}

/* 自定义图标样式 */
.custom-icon {
  width: 24px;
  height: 24px;
  color: white;
  filter: drop-shadow(0 2px 4px rgba(0, 0, 0, 0.2));
  pointer-events: none;
}

/* 设置图标 - 9点网格样式 */
.settings-icon {
  background: linear-gradient(135deg, #6c757d, #495057) !important;
}

/* 关闭按钮样式 */
.close-icon {
  background: linear-gradient(135deg, #ef4444, #dc2626) !important;
}

.close-item:hover .dock-icon {
  transform: scale(1.15);
  box-shadow: 0 4px 12px rgba(239, 68, 68, 0.4);
}

.grid-dots {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 2px;
  width: 16px;
  height: 16px;
}

.grid-dots span {
  width: 4px;
  height: 4px;
  background: white;
  border-radius: 50%;
  opacity: 0.9;
}

/* 徽章 - 精简样式 */
.dock-badge {
  position: absolute;
  top: 2px;
  right: 2px;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  z-index: 2;
  pointer-events: none;
}

.dock-badge.new {
  background: #10b981;
  box-shadow: 0 0 4px rgba(16, 185, 129, 0.6);
}

.dock-badge.building {
  background: #9ca3af;
  box-shadow: 0 0 4px rgba(156, 163, 175, 0.6);
}

.dock-badge.building svg {
  display: none;
}

/* 红色角标 NEW */
.dock-corner-badge {
  position: absolute;
  top: 4px;
  right: 4px;
  padding: 2px 5px;
  background: linear-gradient(135deg, #ef4444, #dc2626);
  color: #fff;
  font-size: 9px;
  font-weight: 700;
  border-radius: 4px;
  z-index: 10;
  pointer-events: none;
  box-shadow: 0 2px 8px rgba(239, 68, 68, 0.5);
  animation: cornerBadgePulse 2s ease-in-out infinite;
  letter-spacing: 0.5px;
}

@keyframes cornerBadgePulse {
  0%, 100% {
    transform: scale(1);
    box-shadow: 0 2px 8px rgba(239, 68, 68, 0.5);
  }
  50% {
    transform: scale(1.05);
    box-shadow: 0 3px 12px rgba(239, 68, 68, 0.7);
  }
}

/* 标签文字 */
.dock-label {
  margin-top: 4px;
  font-size: 12px;
  font-weight: 600;
  color: #ffffff;
  text-align: center;
  white-space: nowrap;
  transition: color 0.2s ease;
  pointer-events: none;
}

.dock-item:hover .dock-label {
  color: #ffffff;
}

.dock-item.disabled .dock-label {
  color: rgba(255, 255, 255, 0.6);
}

/* 明亮主题样式 */
.light-theme {
  background: transparent !important;
}

.light-theme .dock {
  /* 背景色由动态样式控制，支持透明度调节 */
}

.light-theme .dock-divider {
  background: rgba(0, 0, 0, 0.15);
}

.light-theme .dock-label {
  color: #000000;
}

.light-theme .dock-item:hover .dock-label {
  color: #000000;
}

.light-theme .dock-item.disabled .dock-label {
  color: rgba(0, 0, 0, 0.5);
}

/* 确认弹窗样式 - Dock风格 */
.confirm-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.3);
  backdrop-filter: blur(4px);
  -webkit-backdrop-filter: blur(4px);
  border-radius: 16px !important;
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 9999;
}

.confirm-modal {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 14px 20px;
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border-radius: 16px !important;
  overflow: hidden;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.3);
  max-width: 380px;
  animation: confirmPopIn 0.25s cubic-bezier(0.34, 1.56, 0.64, 1);
  -webkit-mask-image: -webkit-radial-gradient(white, black);
}

@keyframes confirmPopIn {
  from {
    opacity: 0;
    transform: scale(0.9);
  }
  to {
    opacity: 1;
    transform: scale(1);
  }
}

.confirm-icon {
  width: 36px;
  height: 36px;
  border-radius: 10px;
  background: linear-gradient(135deg, #f59e0b, #d97706);
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.confirm-icon svg {
  width: 20px;
  height: 20px;
  color: white;
  filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.2));
}

.confirm-content {
  flex: 1;
  min-width: 0;
}

.confirm-title {
  font-size: 14px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.95);
  margin-bottom: 4px;
}

.confirm-message {
  font-size: 12px;
  color: rgba(255, 255, 255, 0.7);
  line-height: 1.5;
  white-space: pre-line;
}

.confirm-actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}

.confirm-btn {
  padding: 6px 14px;
  border-radius: 8px;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s ease;
  border: none;
  outline: none;
}

.confirm-btn.cancel {
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.8);
}

.confirm-btn.cancel:hover {
  background: rgba(255, 255, 255, 0.2);
  color: rgba(255, 255, 255, 1);
}

.confirm-btn.danger {
  background: linear-gradient(135deg, #ef4444, #dc2626);
  color: white;
  box-shadow: 0 2px 8px rgba(239, 68, 68, 0.3);
}

.confirm-btn.danger:hover {
  background: linear-gradient(135deg, #f87171, #ef4444);
  box-shadow: 0 4px 12px rgba(239, 68, 68, 0.4);
  transform: translateY(-1px);
}

.confirm-btn.danger:active {
  transform: translateY(0);
}

/* 明亮主题确认弹窗 - 使用纯色背景，移除毛玻璃效果 */
.confirm-modal.light-theme {
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.25), 0 4px 12px rgba(0, 0, 0, 0.15);
  backdrop-filter: none !important;
  -webkit-backdrop-filter: none !important;
  background: #ffffff !important;
  -webkit-mask-image: none !important;
}

.confirm-modal.light-theme .confirm-title {
  color: rgba(0, 0, 0, 0.85);
}

.confirm-modal.light-theme .confirm-message {
  color: rgba(0, 0, 0, 0.6);
}

.confirm-modal.light-theme .confirm-btn.cancel {
  background: rgba(0, 0, 0, 0.06);
  color: rgba(0, 0, 0, 0.7);
}

.confirm-modal.light-theme .confirm-btn.cancel:hover {
  background: rgba(0, 0, 0, 0.12);
  color: rgba(0, 0, 0, 0.9);
}

/* 过渡动画 */
.confirm-fade-enter-active,
.confirm-fade-leave-active {
  transition: opacity 0.2s ease;
}

.confirm-fade-enter-active .confirm-modal,
.confirm-fade-leave-active .confirm-modal {
  transition: transform 0.2s ease, opacity 0.2s ease;
}

.confirm-fade-enter-from,
.confirm-fade-leave-to {
  opacity: 0;
}

.confirm-fade-enter-from .confirm-modal,
.confirm-fade-leave-to .confirm-modal {
  transform: scale(0.9);
  opacity: 0;
}
</style>
<style>
html, body {
  overflow: hidden;
  background: transparent !important;
  border-radius: 18px;
  margin: 0;
  padding: 0;
}

#app {
  overflow: hidden;
  background: transparent !important;
  border-radius: 18px;
}

.home {
  overflow: hidden !important;
  background: transparent !important;
  border-radius: 20px !important;
}
</style>
