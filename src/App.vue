<script setup name="app">
import { onMounted } from 'vue'
import { useDatabaseStore } from '@/stores/database'

const databaseStore = useDatabaseStore()

const initDatabase = async () => {
  await databaseStore.checkStatus()

  if (!databaseStore.publicReady) {
    await databaseStore.initPublicDatabase()
  }

  // 启动时不再自动提示设置安全数据库密码
  // 仅在访问钱包管理页面时才按需提示
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
