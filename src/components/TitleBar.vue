<template>
  <!-- 自定义标题栏 -->
  <div class="title-bar" @mousedown="handleDragStart">
    <div class="title-bar-left">
      <img src="/app-icon.png" alt="Logo" class="title-bar-logo" />
      
      <template v-if="isEditing">
        <input
          ref="editInputRef"
          v-model="editTitle"
          class="title-bar-input"
          :placeholder="defaultTitle"
          @blur="saveTitle"
          @keydown="handleKeydown"
          @click.stop
        />
      </template>
      
      <template v-else>
        <div
          class="title-wrapper"
          @click="startEditing"
          @mouseenter="isHovered = true"
          @mouseleave="isHovered = false"
        >
          <div
            class="title-bar-text"
            :class="{ 'title-editable': true, 'title-hovered': isHovered && !isCustom }"
            title="点击编辑窗口名称"
          >
            {{ displayTitle }}
          </div>
          
          <span
            v-if="isCustom && isHovered"
            class="reset-button"
            @click.stop="resetToDefault"
            title="恢复默认名称"
          >
            重置
          </span>
          
          <span v-if="isCustom" class="custom-badge" title="已自定义名称">已改</span>
        </div>
        
        <!-- 生态标识 - 可点击切换 -->
        <a-dropdown
          v-if="ecosystem && canSwitchEcosystem"
          trigger="click"
          position="bottom"
          @select="(val) => switchEcosystem(val)"
        >
          <div class="ecosystem-badge clickable" :class="ecosystem.toLowerCase()">
            <span class="ecosystem-badge-icon">
              <!-- Ethereum Icon - 与 EcosystemEntry.vue 一致 -->
              <svg v-if="ecosystem.toLowerCase() === 'evm'" viewBox="0 0 115 182" class="chain-icon-svg evm-icon">
                <path fill="#F0CDC2" stroke="#8B5CF6" stroke-width="2" d="M57.505 181v-45.16L1.641 103.171z"/>
                <path fill="#C9B3F5" stroke="#8B5CF6" stroke-width="2" d="M57.69 181v-45.16l55.865-32.669z"/>
                <path fill="#88AAF1" stroke="#8B5CF6" stroke-width="2" d="M57.506 124.615V66.979L1 92.28z"/>
                <path fill="#C9B3F5" stroke="#8B5CF6" stroke-width="2" d="M57.69 124.615V66.979l56.506 25.302z"/>
                <path fill="#F0CDC2" stroke="#8B5CF6" stroke-width="2" d="M1 92.281 57.505 1v65.979z"/>
                <path fill="#B8FAF6" stroke="#8B5CF6" stroke-width="2" d="M114.196 92.281 57.691 1v65.979z"/>
              </svg>
              <!-- Solana Icon - 与 EcosystemEntry.vue 一致 -->
              <svg v-else-if="ecosystem.toLowerCase() === 'solana'" viewBox="0 0 397.7 311.7" class="chain-icon-svg solana-icon">
                <path d="M64.6 237.9c2.4-2.4 5.7-3.8 9.2-3.8h317.4c5.8 0 8.7 7 4.6 11.1l-62.7 62.7c-2.4 2.4-5.7 3.8-9.2 3.8H6.5c-5.8 0-8.7-7-4.6-11.1l62.7-62.7z" fill="#14F195"/>
                <path d="M64.6 3.8C67.1 1.4 70.4 0 73.8 0h317.4c5.8 0 8.7 7 4.6 11.1l-62.7 62.7c-2.4 2.4-5.7 3.8-9.2 3.8H6.5c-5.8 0-8.7-7-4.6-11.1L64.6 3.8z" fill="#9945FF"/>
                <path d="M333.1 120.1c-2.4-2.4-5.7-3.8-9.2-3.8H6.5c-5.8 0-8.7 7-4.6 11.1l62.7 62.7c2.4 2.4 5.7 3.8 9.2 3.8h317.4c5.8 0 8.7-7 4.6-11.1l-62.7-62.7z" fill="#14F195"/>
              </svg>
            </span>
            <span class="ecosystem-badge-text">{{ getEcosystemLabel(ecosystem) }}</span>
            <icon-down class="ecosystem-arrow" />
          </div>
          <template #content>
            <a-doption
              v-for="opt in ecosystemOptions"
              :key="opt.value"
              :value="opt.value"
              :class="{ 'active': ecosystem.toLowerCase() === opt.value }"
            >
              <span class="ecosystem-option">
                <span class="ecosystem-option-icon">
                  <!-- Ethereum Icon - 与 EcosystemEntry.vue 一致 -->
                  <svg v-if="opt.value === 'evm'" viewBox="0 0 115 182" class="chain-icon-svg evm-icon">
                    <path fill="#F0CDC2" stroke="#8B5CF6" stroke-width="2" d="M57.505 181v-45.16L1.641 103.171z"/>
                    <path fill="#C9B3F5" stroke="#8B5CF6" stroke-width="2" d="M57.69 181v-45.16l55.865-32.669z"/>
                    <path fill="#88AAF1" stroke="#8B5CF6" stroke-width="2" d="M57.506 124.615V66.979L1 92.28z"/>
                    <path fill="#C9B3F5" stroke="#8B5CF6" stroke-width="2" d="M57.69 124.615V66.979l56.506 25.302z"/>
                    <path fill="#F0CDC2" stroke="#8B5CF6" stroke-width="2" d="M1 92.281 57.505 1v65.979z"/>
                    <path fill="#B8FAF6" stroke="#8B5CF6" stroke-width="2" d="M114.196 92.281 57.691 1v65.979z"/>
                  </svg>
                  <!-- Solana Icon - 与 EcosystemEntry.vue 一致 -->
                  <svg v-else-if="opt.value === 'solana'" viewBox="0 0 397.7 311.7" class="chain-icon-svg solana-icon">
                    <path d="M64.6 237.9c2.4-2.4 5.7-3.8 9.2-3.8h317.4c5.8 0 8.7 7 4.6 11.1l-62.7 62.7c-2.4 2.4-5.7 3.8-9.2 3.8H6.5c-5.8 0-8.7-7-4.6-11.1l62.7-62.7z" fill="#14F195"/>
                    <path d="M64.6 3.8C67.1 1.4 70.4 0 73.8 0h317.4c5.8 0 8.7 7 4.6 11.1l-62.7 62.7c-2.4 2.4-5.7 3.8-9.2 3.8H6.5c-5.8 0-8.7-7-4.6-11.1L64.6 3.8z" fill="#9945FF"/>
                    <path d="M333.1 120.1c-2.4-2.4-5.7-3.8-9.2-3.8H6.5c-5.8 0-8.7 7-4.6 11.1l62.7 62.7c2.4 2.4 5.7 3.8 9.2 3.8h317.4c5.8 0 8.7-7 4.6-11.1l-62.7-62.7z" fill="#14F195"/>
                  </svg>
                </span>
                <span class="ecosystem-label">{{ opt.label }}</span>
                <icon-check v-if="ecosystem.toLowerCase() === opt.value" class="check-mark" />
              </span>
            </a-doption>
          </template>
        </a-dropdown>
        <div v-else-if="ecosystem" class="ecosystem-badge" :class="ecosystem.toLowerCase()">
          <span class="ecosystem-badge-icon">
            <!-- Ethereum Icon - 与 EcosystemEntry.vue 一致 -->
            <svg v-if="ecosystem.toLowerCase() === 'evm'" viewBox="0 0 115 182" class="chain-icon-svg evm-icon">
              <path fill="#F0CDC2" stroke="#8B5CF6" stroke-width="2" d="M57.505 181v-45.16L1.641 103.171z"/>
              <path fill="#C9B3F5" stroke="#8B5CF6" stroke-width="2" d="M57.69 181v-45.16l55.865-32.669z"/>
              <path fill="#88AAF1" stroke="#8B5CF6" stroke-width="2" d="M57.506 124.615V66.979L1 92.28z"/>
              <path fill="#C9B3F5" stroke="#8B5CF6" stroke-width="2" d="M57.69 124.615V66.979l56.506 25.302z"/>
              <path fill="#F0CDC2" stroke="#8B5CF6" stroke-width="2" d="M1 92.281 57.505 1v65.979z"/>
              <path fill="#B8FAF6" stroke="#8B5CF6" stroke-width="2" d="M114.196 92.281 57.691 1v65.979z"/>
            </svg>
            <!-- Solana Icon - 与 EcosystemEntry.vue 一致 -->
            <svg v-else-if="ecosystem.toLowerCase() === 'solana'" viewBox="0 0 397.7 311.7" class="chain-icon-svg solana-icon">
              <path d="M64.6 237.9c2.4-2.4 5.7-3.8 9.2-3.8h317.4c5.8 0 8.7 7 4.6 11.1l-62.7 62.7c-2.4 2.4-5.7 3.8-9.2 3.8H6.5c-5.8 0-8.7-7-4.6-11.1l62.7-62.7z" fill="#14F195"/>
              <path d="M64.6 3.8C67.1 1.4 70.4 0 73.8 0h317.4c5.8 0 8.7 7 4.6 11.1l-62.7 62.7c-2.4 2.4-5.7 3.8-9.2 3.8H6.5c-5.8 0-8.7-7-4.6-11.1L64.6 3.8z" fill="#9945FF"/>
              <path d="M333.1 120.1c-2.4-2.4-5.7-3.8-9.2-3.8H6.5c-5.8 0-8.7 7-4.6 11.1l62.7 62.7c2.4 2.4 5.7 3.8 9.2 3.8h317.4c5.8 0 8.7-7 4.6-11.1l-62.7-62.7z" fill="#14F195"/>
            </svg>
          </span>
          <span class="ecosystem-badge-text">{{ getEcosystemLabel(ecosystem) }}</span>
        </div>
      </template>
    </div>
    
    <div class="title-bar-controls">
      <!-- 主题切换开关 -->
      <div class="titlebar-center">
        <div class="theme-toggle-container">
          <span class="theme-icon-wrapper" :class="{ 'active': !isDarkTheme }">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="theme-icon-svg">
              <circle cx="12" cy="12" r="5" />
              <path d="M12 1v2M12 21v2M4.22 4.22l1.42 1.42M18.36 18.36l1.42 1.42M1 12h2M21 12h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42" />
            </svg>
          </span>
          <a-switch v-model="isDarkTheme" size="small" class="theme-switch" />
          <span class="theme-icon-wrapper" :class="{ 'active': isDarkTheme }">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="theme-icon-svg">
              <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
            </svg>
          </span>
        </div>
      </div>
      <button class="title-bar-control" @click="minimizeWindow" title="最小化">
        <svg width="12" height="12" viewBox="0 0 12 12">
          <path d="M2 6h8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
        </svg>
      </button>
      <button class="title-bar-control" @click="maximizeWindow" :title="isMaximized ? '还原' : '最大化'">
        <svg v-if="!isMaximized" width="12" height="12" viewBox="0 0 12 12">
          <rect x="2" y="2" width="8" height="8" stroke="currentColor" stroke-width="1.5" fill="none" stroke-linecap="round" />
        </svg>
        <svg v-else width="12" height="12" viewBox="0 0 12 12">
          <rect x="3" y="1" width="6" height="6" stroke="currentColor" stroke-width="1.5" fill="none" stroke-linecap="round" />
          <rect x="1" y="3" width="6" height="6" stroke="currentColor" stroke-width="1.5" fill="none" stroke-linecap="round" />
        </svg>
      </button>
<button
          class="title-bar-control close"
          :class="{ 'disabled': disableClose }"
          @click="closeWindow"
          :title="disableClose ? '操作进行中，无法关闭' : '关闭'"
          :style="{ cursor: disableClose ? 'not-allowed' : 'pointer', opacity: disableClose ? 0.5 : 1 }"
        >
        <svg width="12" height="12" viewBox="0 0 12 12">
          <path d="M3 3l6 6M9 3l-6 6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
        </svg>
      </button>
    </div>
  </div>
</template>

<script setup>
import { computed, onMounted, onUnmounted, ref, nextTick, watch } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { Notification, Dropdown, Button, Space } from '@arco-design/web-vue'
import { IconDown, IconCheck } from '@arco-design/web-vue/es/icon'
import { useThemeStore } from '@/stores'
import { useEcosystemStore } from '@/stores/ecosystem'
import { useRoute, useRouter } from 'vue-router'
import { WINDOW_CONFIG } from '@/utils/windowNames'

const isMaximized = ref(false)
const isEditing = ref(false)
const isHovered = ref(false)
const editTitle = ref('')
const editInputRef = ref(null)
const customTitle = ref(null)
const showEcosystemDropdown = ref(false)

const props = defineProps({
  title: {
    type: String,
    default: 'WalletsTool'
  },
  windowLabel: {
    type: String,
    default: ''
  },
  customClose: {
    type: Boolean,
    default: false
  },
  disableClose: {
    type: Boolean,
    default: false
  },
  ecosystem: {
    type: String,
    default: ''
  }
})

const emit = defineEmits(['before-close', 'title-changed'])

const themeStore = useThemeStore()
const ecoStore = useEcosystemStore()
const router = useRouter()
const route = useRoute()
const currentTheme = computed(() => themeStore.currentTheme)

// 切换主题
function toggleTheme() {
  themeStore.toggleTheme()
}

const isDarkTheme = computed({
  get: () => themeStore.getEffectiveTheme() === 'dark',
  set: (value) => {
    // 只在值真正改变时才切换主题
    const currentIsDark = themeStore.getEffectiveTheme() === 'dark'
    if (value !== currentIsDark) {
      toggleTheme()
    }
  }
})

const defaultTitle = computed(() => props.title || 'WalletsTool')

const displayTitle = computed(() => {
  if (customTitle.value) {
    return customTitle.value
  }
  return defaultTitle.value
})

const isCustom = computed(() => {
  return customTitle.value !== null
})

function getWindowLabel() {
  if (props.windowLabel) {
    return props.windowLabel
  }
  try {
    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
    if (isTauri) {
      return getCurrentWindow().label
    }
  } catch (e) {
    console.error('获取窗口标签失败:', e)
  }
  return 'main'
}

async function loadCustomTitle() {
  const label = getWindowLabel()
  const saved = WINDOW_CONFIG.getCustomTitle(label)
  if (saved) {
    customTitle.value = saved
    try {
      const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
      if (isTauri) {
        const currentWindow = getCurrentWindow()
        await currentWindow.setTitle(saved)
      }
    } catch (e) {
      console.error('设置窗口标题失败:', e)
    }
  }
}

async function startEditing() {
  isEditing.value = true
  editTitle.value = customTitle.value || defaultTitle.value
  
  await nextTick()
  
  if (editInputRef.value) {
    editInputRef.value.focus()
    editInputRef.value.select()
  }
}

async function saveTitle() {
  const trimmedTitle = editTitle.value.trim()
  
  if (!trimmedTitle) {
    cancelEdit()
    return
  }
  
  if (trimmedTitle.length > 50) {
    Notification.warning({
      content: '窗口名称不能超过50个字符',
      position: 'top'
    })
    editInputRef.value?.focus()
    return
  }
  
  const label = getWindowLabel()
  const newTitle = trimmedTitle
  
  if (WINDOW_CONFIG.saveCustomTitle(label, newTitle)) {
    customTitle.value = newTitle
    
    try {
      const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
      if (isTauri) {
        const currentWindow = getCurrentWindow()
        await currentWindow.setTitle(newTitle)
      }
    } catch (e) {
      console.error('设置窗口标题失败:', e)
    }
    
    emit('title-changed', newTitle)
  }
  
  isEditing.value = false
}

async function resetToDefault() {
  const label = getWindowLabel()
  
  if (WINDOW_CONFIG.removeCustomTitle(label)) {
    customTitle.value = null
    
    try {
      const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
      if (isTauri) {
        const currentWindow = getCurrentWindow()
        await currentWindow.setTitle(defaultTitle.value)
      }
    } catch (e) {
      console.error('恢复默认窗口标题失败:', e)
    }
    
    emit('title-changed', defaultTitle.value)
  }
}

function cancelEdit() {
  editTitle.value = ''
  isEditing.value = false
}

function handleKeydown(event) {
  if (event.key === 'Enter') {
    event.preventDefault()
    saveTitle()
  } else if (event.key === 'Escape') {
    event.preventDefault()
    cancelEdit()
  }
}

async function handleDragStart(event) {
  if (event.button !== 0) return;
  if (event.target.closest('.title-bar-controls, button, input, .ecosystem-badge.clickable, .theme-toggle-container')) return;
  const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__;
  if (isTauri) {
    try {
      await getCurrentWindow().startDragging();
    } catch (e) {}
  }
}

async function minimizeWindow() {
  const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
  if (isTauri) {
    try {
      const currentWindow = getCurrentWindow()
      await currentWindow.minimize()
    } catch (error) {
      console.error('Error minimizing window:', error)
    }
  }
}

async function maximizeWindow() {
  const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
  if (isTauri) {
    try {
      const currentWindow = getCurrentWindow()
      const currentMaximized = await currentWindow.isMaximized()
      if (currentMaximized) {
        await currentWindow.unmaximize()
        isMaximized.value = false
      } else {
        await currentWindow.maximize()
        isMaximized.value = true
      }
    } catch (error) {
      console.error('Error toggling maximize window:', error)
    }
  }
}

async function closeWindow() {
  if (props.disableClose) {
    return
  }

  const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
  if (isTauri) {
    try {
      console.log('TitleBar窗口关闭事件触发，正在通知父组件执行清理操作...')

      emit('before-close')

      if (props.customClose) {
        return
      }

      const currentWindow = getCurrentWindow()
      await currentWindow.destroy()
    } catch (error) {
      console.error('Error closing window:', error)
    }
  }
}

let unlistenResize = null

onMounted(async () => {
  const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
  if (isTauri) {
    try {
      const currentWindow = getCurrentWindow()
      isMaximized.value = await currentWindow.isMaximized()
      
      // 监听窗口大小变化，同步最大化状态
      unlistenResize = await currentWindow.onResized(async () => {
        isMaximized.value = await currentWindow.isMaximized()
      })
      
      await loadCustomTitle()
    } catch (error) {
      console.error('Error getting window state:', error)
    }
  }
})

watch(() => props.title, (newTitle) => {
  if (!customTitle.value && newTitle) {
  }
})

onUnmounted(() => {
  if (unlistenResize) {
    unlistenResize()
  }
})

// 生态切换相关
const ecosystemOptions = [
  { label: 'Ethereum', value: 'evm' },
  { label: 'Solana', value: 'solana' }
]

// 获取生态显示名称
const getEcosystemLabel = (eco) => {
  const lowerEco = eco?.toLowerCase()
  if (lowerEco === 'evm') return 'Ethereum'
  if (lowerEco === 'solana') return 'Solana'
  return eco
}

// 当前页面功能映射
const pageMap = {
  evm: {
    transfer: '/eth/transfer',
    balance: '/eth/balance',
    monitor: '/eth/monitor'
  },
  solana: {
    transfer: '/sol/transfer',
    balance: '/sol/balance'
  }
}

// 判断当前页面功能
const getCurrentPageFeature = () => {
  const path = route.path
  if (path.includes('/transfer')) return 'transfer'
  if (path.includes('/balance')) return 'balance'
  if (path.includes('/monitor')) return 'monitor'
  return null
}

// 判断是否显示生态切换（只在 transfer/balance/monitor 页面显示）
const canSwitchEcosystem = computed(() => {
  const path = route.path
  return path.includes('/eth/') || path.includes('/sol/')
})

// 切换生态
const switchEcosystem = (targetEco) => {
  const currentEco = props.ecosystem?.toLowerCase()
  if (targetEco === currentEco) return

  const feature = getCurrentPageFeature()
  if (!feature) return

  // 检查目标生态是否支持当前功能
  const targetPath = pageMap[targetEco]?.[feature]
  if (!targetPath) {
    Notification.warning({
      title: '暂不支持',
      content: `${targetEco === 'solana' ? 'Solana' : 'EVM'} 生态暂不支持此功能`,
      position: 'top',
    })
    return
  }

  // 更新生态状态
  ecoStore.setEco(targetEco === 'evm' ? 'eth' : 'sol')

  // 跳转到对应生态的页面
  router.replace(targetPath)
}
</script>

<style scoped>
.title-bar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  height: 40px;
  background: linear-gradient(135deg, #161b22 0%, #0d1117 100%);
  color: var(--color-text-1, #e8eaf6);
  font-size: 14px;
  -webkit-app-region: drag;
  user-select: none;
  backdrop-filter: blur(10px);
  border-bottom: 1px solid rgba(88, 108, 199, 0.2);
  padding: 0 10px;
  font-weight: 500;
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  z-index: 1000;
  border-radius: 12px 12px 0 0;
}

.title-bar-left {
  display: flex;
  align-items: center;
  margin-left: 10px;
}

.title-bar-logo {
  width: 24px;
  height: 24px;
  margin-right: 8px;
  border-radius: 4px;
  object-fit: contain;
}

.title-bar-text {
  font-weight: 500;
  font-size: 14px;
}

.title-wrapper {
  display: flex;
  align-items: center;
  gap: 4px;
}

.title-bar-input {
  display: inline-block;
  background: rgba(13, 17, 23, 0.8);
  border: 1px solid rgba(88, 108, 199, 0.3);
  border-radius: 4px;
  padding: 4px 8px;
  font-size: 14px;
  font-weight: 500;
  color: inherit;
  outline: none;
  line-height: 1.2;
  width: auto;
  min-width: 120px;
  max-width: 300px;
  -webkit-app-region: no-drag;
  transition: all 0.2s ease;
  box-sizing: border-box;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  position: relative;
  z-index: 10;
}

.title-bar-input:focus {
  border-color: rgba(91, 138, 255, 0.8);
  background: rgba(13, 17, 23, 1);
  box-shadow: 0 0 0 2px rgba(91, 138, 255, 0.2);
}

.title-bar-input::placeholder {
  color: var(--color-text-3, #9aa3c2);
}

.custom-badge {
  font-size: 10px;
  color: #fbbf24;
  opacity: 0.8;
  transition: opacity 0.2s ease;
  white-space: nowrap;
}

.custom-badge:hover {
  opacity: 1;
}

.reset-button {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0 6px;
  height: 18px;
  background: rgba(96, 165, 250, 0.8);
  border-radius: 4px;
  color: white;
  font-size: 10px;
  cursor: pointer;
  -webkit-app-region: no-drag;
  transition: all 0.2s ease;
  margin-left: 4px;
  white-space: nowrap;
}

.reset-button:hover {
  background: rgba(59, 130, 246, 0.9);
}

.ecosystem-badge {
  margin-left: 12px;
  padding: 4px 10px 4px 6px;
  border-radius: 20px;
  font-size: 12px;
  font-weight: 500;
  -webkit-app-region: no-drag;
  cursor: default;
  display: flex;
  align-items: center;
  gap: 6px;
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(255, 255, 255, 0.1);
  backdrop-filter: blur(10px);
  transition: all 0.2s ease;
}

.ecosystem-badge.clickable {
  cursor: pointer;
}

.ecosystem-badge.clickable:hover {
  background: rgba(255, 255, 255, 0.15);
  border-color: rgba(255, 255, 255, 0.2);
  transform: translateY(-1px);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
}

.ecosystem-badge-icon {
  width: 20px;
  height: 20px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  overflow: hidden;
  background: rgba(255, 255, 255, 0.9);
  padding: 2px;
}

.ecosystem-badge-icon img,
.chain-icon-svg {
  width: 100%;
  height: 100%;
  object-fit: contain;
}

/* EVM 图标样式 - 与 EcosystemEntry 一致 */
.evm-icon {
  width: 14px;
  height: 14px;
}

/* Solana 图标样式 */
.solana-icon {
  width: 16px;
  height: 16px;
}

.ecosystem-badge-text {
  color: var(--color-text-1, #e8eaf6);
}

.ecosystem-arrow {
  font-size: 10px;
  opacity: 0.7;
  transition: transform 0.2s ease;
  color: var(--color-text-2, #c9d1d9);
}

.ecosystem-badge.clickable:hover .ecosystem-arrow {
  transform: rotate(180deg);
  opacity: 1;
}

/* 下拉菜单样式 */
.ecosystem-option {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 0;
  white-space: nowrap;
}

.ecosystem-option-icon {
  width: 22px;
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  overflow: hidden;
  background: white;
  padding: 2px;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
}

.ecosystem-option-icon img,
.ecosystem-option-icon .chain-icon-svg {
  width: 100%;
  height: 100%;
  object-fit: contain;
}

/* 下拉菜单中的图标尺寸调整 */
.ecosystem-option-icon .evm-icon {
  width: 18px;
  height: 18px;
}

.ecosystem-option-icon .solana-icon {
  width: 20px;
  height: 20px;
}

.ecosystem-label {
  flex: 1;
  font-size: 13px;
  font-weight: 500;
  color: var(--color-text-1, #e8eaf6);
}

.check-mark {
  color: var(--color-primary, #3b82f6);
  font-size: 12px;
}

:deep(.arco-dropdown-option) {
  padding: 10px 14px;
  border-radius: 10px;
  transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
  border: 1px solid transparent;
}

:deep(.arco-dropdown-option:hover) {
  background: linear-gradient(135deg, rgba(88, 108, 199, 0.2) 0%, rgba(59, 130, 246, 0.15) 100%);
  border-color: rgba(88, 108, 199, 0.3);
  transform: translateX(2px);
}

:deep(.arco-dropdown-option.active) {
  background: linear-gradient(135deg, rgba(59, 130, 246, 0.25) 0%, rgba(37, 99, 235, 0.2) 100%);
  border-color: rgba(59, 130, 246, 0.4);
}

:deep(.arco-dropdown) {
  padding: 6px;
  border-radius: 12px;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4), 0 0 0 1px rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(255, 255, 255, 0.12);
  backdrop-filter: blur(20px);
  width: auto !important;
  min-width: unset !important;
  background: linear-gradient(145deg, rgba(30, 35, 48, 0.95) 0%, rgba(20, 24, 35, 0.98) 100%);
}

:deep(.arco-dropdown-list) {
  width: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

/* 生态特定样式 */
.ecosystem-badge.evm {
  background: linear-gradient(135deg, rgba(59, 130, 246, 0.15) 0%, rgba(37, 99, 235, 0.1) 100%);
  border-color: rgba(59, 130, 246, 0.3);
}

.ecosystem-badge.evm:hover {
  background: linear-gradient(135deg, rgba(59, 130, 246, 0.25) 0%, rgba(37, 99, 235, 0.2) 100%);
  border-color: rgba(59, 130, 246, 0.5);
}

.ecosystem-badge.solana {
  background: linear-gradient(135deg, rgba(153, 69, 255, 0.15) 0%, rgba(20, 241, 149, 0.1) 100%);
  border-color: rgba(153, 69, 255, 0.3);
}

.ecosystem-badge.solana:hover {
  background: linear-gradient(135deg, rgba(153, 69, 255, 0.25) 0%, rgba(20, 241, 149, 0.2) 100%);
  border-color: rgba(153, 69, 255, 0.5);
}

.title-editable {
  cursor: text;
  padding: 2px 6px;
  border-radius: 4px;
  transition: all 0.2s ease;
  border: 1px solid transparent;
}

.title-editable.title-hovered {
  background: rgba(88, 108, 199, 0.15);
  border-color: rgba(88, 108, 199, 0.25);
}

.title-editable.title-hovered:hover {
  background: rgba(88, 108, 199, 0.25);
  border-color: rgba(91, 138, 255, 0.4);
}

.title-bar-controls {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 100%;
  -webkit-app-region: no-drag;
}

.title-bar-control {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  background: rgba(88, 108, 199, 0.15);
  border: none;
  border-radius: 6px;
  color: var(--color-text-2, #c9d1d9);
  cursor: pointer;
  font-size: 16px;
  transition: all 0.2s ease;
  -webkit-app-region: no-drag;
}

.title-bar-control:hover {
  background: rgba(88, 108, 199, 0.3);
  color: var(--color-text-1, #e8eaf6);
}

.title-bar-control.close:hover {
  background-color: rgba(220, 38, 38, 0.8) !important;
  color: white !important;
}

.title-bar-control.theme-toggle {
  width: 40px;
  margin-right: 5px;
}

.title-bar-control.close:hover {
  background: rgba(96, 96, 96, 0.9);
  color: white;
}

.title-bar-control:first-of-type:hover {
  background: rgba(206, 184, 136, 0.8);
  color: white;
}

.title-bar-control:nth-of-type(2):hover {
  background: rgba(73, 152, 220, 0.8);
  color: white;
}

.titlebar-center {
  position: absolute;
  left: 50%;
  top: 50%;
  transform: translate(-50%, -50%);
  z-index: 1002;
}

.theme-toggle-container {
  display: flex;
  align-items: center;
  gap: 6px;
  background: rgba(255, 255, 255, 0.08);
  border-radius: 20px;
  padding: 4px 10px 4px 6px;
  backdrop-filter: blur(10px);
  border: 1px solid rgba(255, 255, 255, 0.1);
  -webkit-app-region: no-drag;
  transition: all 0.2s ease;
  cursor: pointer;
}

.theme-toggle-container:hover {
  background: rgba(255, 255, 255, 0.15);
  border-color: rgba(255, 255, 255, 0.2);
  transform: translateY(-1px);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
}

.theme-icon-wrapper {
  width: 18px;
  height: 18px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  overflow: hidden;
  background: white;
  padding: 2px;
  transition: all 0.2s ease;
}

.theme-icon-wrapper.active {
  background: linear-gradient(135deg, #3b82f6 0%, #2563eb 100%);
  box-shadow: 0 2px 8px rgba(59, 130, 246, 0.4);
}

.theme-icon-wrapper.active .theme-icon-svg {
  color: white;
}

.theme-icon-svg {
  width: 100%;
  height: 100%;
  color: #4a5568;
  transition: all 0.2s ease;
}

.theme-toggle-container:hover .theme-icon-svg {
  color: #1a202c;
}

.theme-switch {
  margin: 0 4px;
}

.title-bar-control .iconify {
  opacity: 0.9;
  transition: opacity 0.2s ease;
}

.title-bar-control:hover .iconify {
  opacity: 1;
}

/* 明亮主题样式 */
:root[data-theme="light"] .title-bar {
  background: linear-gradient(135deg, #f8fafc 0%, #e2e8f0 100%);
  color: #1a202c;
  border-bottom: 1px solid rgba(0, 0, 0, 0.1);
}

:root[data-theme="light"] .title-bar-control {
  color: #4a5568;
}

:root[data-theme="light"] .title-bar-control:hover {
  background-color: rgba(0, 0, 0, 0.1);
}

:root[data-theme="light"] .title-bar-control.close:hover {
  background-color: #e53e3e;
  color: white;
}

:root[data-theme="light"] .title-switch :deep(.arco-switch) {
  background-color: rgba(0, 0, 0, 0.1);
  border: 1px solid rgba(0, 0, 0, 0.2);
}

:root[data-theme="light"] .title-switch :deep(.arco-switch-checked) {
  background-color: rgba(0, 0, 0, 0.2);
}

:root[data-theme="light"] .title-switch :deep(.arco-switch-dot) {
  background-color: #4a5568;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
}

:root[data-theme="light"] .theme-toggle-container {
  background: rgba(0, 0, 0, 0.04);
  border: 1px solid rgba(0, 0, 0, 0.08);
}

:root[data-theme="light"] .theme-toggle-container:hover {
  background: rgba(0, 0, 0, 0.08);
  border-color: rgba(0, 0, 0, 0.15);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.1);
}

:root[data-theme="light"] .theme-icon-svg {
  color: rgba(0, 0, 0, 0.6);
}

:root[data-theme="light"] .theme-toggle-container:hover .theme-icon-svg {
  color: rgba(0, 0, 0, 0.9);
}

:root[data-theme="light"] .theme-icon-wrapper.active {
  background: linear-gradient(135deg, #3b82f6 0%, #2563eb 100%);
}

:root[data-theme="light"] .theme-icon-wrapper.active .theme-icon-svg {
  color: white;
}

:root[data-theme="light"] .title-bar-input {
  background: rgba(0, 0, 0, 0.05);
  border-color: rgba(0, 0, 0, 0.2);
  color: #1a202c;
}

:root[data-theme="light"] .title-bar-input:focus {
  border-color: rgba(66, 153, 225, 0.8);
  background: rgba(0, 0, 0, 0.08);
}

:root[data-theme="light"] .title-bar-input::placeholder {
  color: rgba(0, 0, 0, 0.4);
}

:root[data-theme="light"] .title-editable.title-hovered {
  background: rgba(0, 0, 0, 0.08);
  border-color: rgba(0, 0, 0, 0.15);
}

:root[data-theme="light"] .title-editable.title-hovered:hover {
  background: rgba(0, 0, 0, 0.12);
  border-color: rgba(0, 0, 0, 0.25);
}

/* 明亮主题下的生态切换按钮样式 */
:root[data-theme="light"] .ecosystem-badge {
  background: rgba(0, 0, 0, 0.04);
  border-color: rgba(0, 0, 0, 0.08);
}

:root[data-theme="light"] .ecosystem-badge-text {
  color: #1a202c;
}

:root[data-theme="light"] .ecosystem-arrow {
  color: #4a5568;
}

:root[data-theme="light"] .ecosystem-badge.clickable:hover {
  background: rgba(0, 0, 0, 0.08);
  border-color: rgba(0, 0, 0, 0.15);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.1);
}

:root[data-theme="light"] .ecosystem-badge.evm {
  background: linear-gradient(135deg, rgba(59, 130, 246, 0.1) 0%, rgba(37, 99, 235, 0.05) 100%);
  border-color: rgba(59, 130, 246, 0.2);
}

:root[data-theme="light"] .ecosystem-badge.evm:hover {
  background: linear-gradient(135deg, rgba(59, 130, 246, 0.2) 0%, rgba(37, 99, 235, 0.1) 100%);
  border-color: rgba(59, 130, 246, 0.4);
}

:root[data-theme="light"] .ecosystem-badge.solana {
  background: linear-gradient(135deg, rgba(153, 69, 255, 0.1) 0%, rgba(20, 241, 149, 0.05) 100%);
  border-color: rgba(153, 69, 255, 0.2);
}

:root[data-theme="light"] .ecosystem-badge.solana:hover {
  background: linear-gradient(135deg, rgba(153, 69, 255, 0.2) 0%, rgba(20, 241, 149, 0.1) 100%);
  border-color: rgba(153, 69, 255, 0.4);
}

:root[data-theme="light"] :deep(.arco-dropdown) {
  background: linear-gradient(145deg, rgba(255, 255, 255, 0.98) 0%, rgba(248, 250, 252, 0.95) 100%);
  border: 1px solid rgba(0, 0, 0, 0.08);
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.15), 0 0 0 1px rgba(0, 0, 0, 0.02);
}

:root[data-theme="light"] .ecosystem-label {
  color: #1a202c;
}

:root[data-theme="light"] :deep(.arco-dropdown-option:hover) {
  background: linear-gradient(135deg, rgba(59, 130, 246, 0.12) 0%, rgba(37, 99, 235, 0.08) 100%);
  border-color: rgba(59, 130, 246, 0.25);
}

:root[data-theme="light"] :deep(.arco-dropdown-option.active) {
  background: linear-gradient(135deg, rgba(59, 130, 246, 0.18) 0%, rgba(37, 99, 235, 0.12) 100%);
  border-color: rgba(59, 130, 246, 0.35);
}
</style>

<!-- 全局样式：Arco Design 下拉菜单被 teleport 到 body，需要非 scoped 样式 -->
<style>
/* 深色主题下拉菜单样式 - 直接匹配所有下拉菜单 */
.arco-dropdown {
  padding: 6px !important;
  border-radius: 12px !important;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4), 0 0 0 1px rgba(255, 255, 255, 0.08) !important;
  border: 1px solid rgba(255, 255, 255, 0.12) !important;
  backdrop-filter: blur(20px) !important;
  background: linear-gradient(145deg, rgba(30, 35, 48, 0.95) 0%, rgba(20, 24, 35, 0.98) 100%) !important;
  overflow: visible !important;
}

.arco-dropdown .arco-dropdown-list {
  display: flex !important;
  flex-direction: column !important;
  gap: 2px !important;
  overflow: visible !important;
  max-height: none !important;
}

/* 隐藏下拉菜单滚动条 */
.arco-dropdown ::-webkit-scrollbar {
  display: none !important;
  width: 0 !important;
  height: 0 !important;
}

.arco-dropdown {
  -ms-overflow-style: none !important;
  scrollbar-width: none !important;
}

.arco-dropdown .arco-dropdown-option {
  padding: 3px 10px !important;
  border-radius: 6px !important;
  transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1) !important;
  border: 1px solid transparent !important;
}

.arco-dropdown .arco-dropdown-option:hover {
  background: linear-gradient(135deg, rgba(88, 108, 199, 0.2) 0%, rgba(59, 130, 246, 0.15) 100%) !important;
  border-color: rgba(88, 108, 199, 0.3) !important;
}

.arco-dropdown .arco-dropdown-option.active {
  background: linear-gradient(135deg, rgba(59, 130, 246, 0.25) 0%, rgba(37, 99, 235, 0.2) 100%) !important;
  border-color: rgba(59, 130, 246, 0.4) !important;
}

/* 浅色主题下拉菜单样式 */
:root[data-theme="light"] .arco-dropdown {
  background: linear-gradient(145deg, rgba(255, 255, 255, 0.98) 0%, rgba(248, 250, 252, 0.95) 100%) !important;
  border: 1px solid rgba(0, 0, 0, 0.08) !important;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.15), 0 0 0 1px rgba(0, 0, 0, 0.02) !important;
  overflow: visible !important;
}

:root[data-theme="light"] .arco-dropdown .arco-dropdown-option:hover {
  background: linear-gradient(135deg, rgba(59, 130, 246, 0.12) 0%, rgba(37, 99, 235, 0.08) 100%) !important;
  border-color: rgba(59, 130, 246, 0.25) !important;
}

:root[data-theme="light"] .arco-dropdown .arco-dropdown-option.active {
  background: linear-gradient(135deg, rgba(59, 130, 246, 0.18) 0%, rgba(37, 99, 235, 0.12) 100%) !important;
  border-color: rgba(59, 130, 246, 0.35) !important;
}
</style>