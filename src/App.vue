<script setup name="app">
import { onMounted } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useDatabaseStore } from '@/stores/database'

const databaseStore = useDatabaseStore()

const initDatabase = async () => {
  const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
  if (!isTauri) return

  // 子窗口跳过数据库初始化（主窗口已完成）
  try {
    const win = getCurrentWindow()
    if (win.label !== 'main') return
  } catch (_) {}

  await databaseStore.checkStatus()

  if (!databaseStore.publicReady) {
    await databaseStore.initPublicDatabase()
  }
}

onMounted(async () => {
  await initDatabase()
  // 更新检查已移至 Home 页面，通过 Dock 设置按钮的 badge 提示
})
</script>

<template>
  <div class="window-root">
    <Suspense>
      <router-view></router-view>
    </Suspense>
  </div>
</template>

<style>
.window-root {
  width: 100%;
  height: 100vh;
  border-radius: 12px;
  overflow: hidden;
  position: relative;
  transform: translateZ(0);
}
</style>
