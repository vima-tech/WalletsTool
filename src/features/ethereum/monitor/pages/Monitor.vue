<script setup name="monitor">
import { ref, reactive, onBeforeMount, onMounted, computed, defineAsyncComponent } from 'vue'
import { Notification } from '@arco-design/web-vue'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import TitleBar from '@/components/TitleBar.vue'
import VirtualScrollerTable from '@/components/VirtualScrollerTable.vue'
import { ethers } from 'ethers'
import { utils as xlUtils, read as xlRead } from 'xlsx'
import { WINDOW_CONFIG } from '@/utils/windowNames'
import { downloadWithDialog, openDirectory } from '@/utils/downloadWithDialog'

// 懒加载组件
const ChainManagement = defineAsyncComponent(() => import('@/components/ChainManagement.vue'))
const RpcManagement = defineAsyncComponent(() => import('@/components/RpcManagement.vue'))
const CodeEditor = defineAsyncComponent(() => import('@/components/CodeEditor.vue'))

const chainManageRef = ref(null)
const rpcManageRef = ref(null)
const uploadInputRef = ref(null)

// 表格列
const columns = [
  { title: '序号', align: 'center', width: 55, slotName: 'index' },
  { title: '钱包地址', align: 'center', width: 390, dataIndex: 'address', ellipsis: true, tooltip: true },
  { title: 'Nonce', align: 'center', width: 80, dataIndex: 'nonce' },
  { title: '平台币余额', align: 'center', width: 120, dataIndex: 'plat_balance' },
  { title: '变化', align: 'center', width: 120, dataIndex: 'change_desc', ellipsis: true, tooltip: true },
  { title: '上次变化时间', align: 'center', width: 160, dataIndex: 'last_change_at' },
  { title: '错误信息', align: 'center', dataIndex: 'error_msg', width: 120, ellipsis: true, tooltip: true },
  { title: '操作', align: 'center', width: 70, slotName: 'optional' },
]

const data = ref([])
const selectedKeys = ref([])
const rowSelection = reactive({
  type: 'checkbox',
  showCheckedAll: true,
  selectedRowKeys: selectedKeys,
  onSelect: (rowKeys) => { selectedKeys.value = rowKeys },
  onSelectAll: (selected) => { selectedKeys.value = selected ? data.value.map(i => i.address) : [] }
})

// 监控控制
const monitoring = ref(false)
const intervalSec = ref(15)
let timer = null
const showProgress = ref(false)

// 窗口标题定义
const windowTitle = ref('链上地址监控');

// 窗口标题初始化
function initMonitorWindowTitle() {
  try {
    const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__
    if (isTauri) {
      const windowLabel = getCurrentWindow().label
      const saved = WINDOW_CONFIG.getCustomTitle(windowLabel)
      if (saved) {
        windowTitle.value = saved
        return
      }
    }
  } catch (e) {
    console.error('初始化监控窗口标题失败:', e)
  }
  
  // 不再设置默认标题，由后端或调用方设置
}

initMonitorWindowTitle()

// 链配置
const chainOptions = ref([])
const chainValue = ref('')
const currentChain = ref(null)
const chainFieldNames = { value: 'key', label: 'scan_url' }

// 导入地址
const importVisible = ref(false)
const importText = ref('')
const importLoading = ref(false)
const addressErrorLines = ref([])
const validationErrors = ref([])

function validateAddress(address) {
  try {
    if (!address || typeof address !== 'string') return false
    const trimmed = address.trim()
    if (!trimmed.startsWith('0x') || trimmed.length !== 42) return false
    const hexPart = trimmed.slice(2)
    if (!/^[0-9a-fA-F]{40}$/.test(hexPart)) return false
    return ethers.isAddress(trimmed)
  } catch { return false }
}

function validateImportData() {
  const addresses = importText.value.split('\n').filter(l => l.trim() !== '')
  validationErrors.value = []
  const errorLines = new Set()
  addresses.forEach((addr, idx) => { if (!validateAddress(addr.trim())) { validationErrors.value.push(`第${idx + 1}行地址格式错误`); errorLines.add(idx + 1) } })
  addressErrorLines.value = Array.from(errorLines)
}

function openImport() { importVisible.value = true }
function cancelImport() {
  if (importLoading.value) return false
  importVisible.value = false
  importText.value = ''
  validationErrors.value = []
  addressErrorLines.value = []
}

async function confirmImport() {
  validateImportData()
  if (validationErrors.value.length > 0) return false
  importLoading.value = true
  try {
    let list = importText.value.split('\n').map(s => s.trim()).filter(Boolean)
    const set = new Set()
    list = list.filter(a => (set.has(a) ? false : (set.add(a), true)))
    const exists = new Set(data.value.map(i => i.address))
    list = list.filter(a => !exists.has(a))
    data.value.push(...list.map(a => ({
      address: a, nonce: '', plat_balance: '', change_desc: '', last_change_at: '', error_msg: '', exec_status: '0'
    })))
    Notification.success({ content: `成功导入 ${list.length} 条地址`, position: 'topLeft' })
    importVisible.value = false
    importText.value = ''
    validationErrors.value = []
    addressErrorLines.value = []
    return true
  } catch (e) {
    Notification.error({ content: '导入失败：' + (e.message || e), position: 'topLeft' })
    return false
  } finally { importLoading.value = false }
}

// 手动录入地址
function handleManualImport() {
  importVisible.value = true;
}

// 上传文件导入
function handleFileUpload() {
  uploadInputRef.value.click();
}

// 下载模板
function downloadTemplate() {
  downloadWithDialog('import_model.xlsx', '导入模板.xlsx').then((path) => {
    if (path) {
      openDirectory(path);
      Notification.success({
        content: '模板已保存',
        position: 'topLeft',
      });
    }
  });
}

// 处理文件变化
function handleFileChange(event) {
  const file = event.target.files[0];
  if (!file) return;

  const reader = new FileReader();
  reader.onload = async (e) => {
    try {
      const data = new Uint8Array(e.target.result);
      const workbook = xlRead(data, { type: 'array' });
      const firstSheet = workbook.Sheets[workbook.SheetNames[0]];
      const jsonData = xlUtils.sheet_to_json(firstSheet, { header: 1 });

      const addresses = [];
      jsonData.forEach((row, index) => {
        if (row && row[0]) {
          const addr = String(row[0]).trim();
          if (validateAddress(addr)) {
            addresses.push(addr);
          }
        }
      });

      if (addresses.length > 0) {
        importText.value = addresses.join('\n');
        validateImportData();
        importVisible.value = true;
        Notification.success({
          content: `成功解析 ${addresses.length} 个地址`,
          position: 'topLeft',
        });
      } else {
        Notification.error({
          content: '未在文件中找到有效的地址数据',
          position: 'topLeft',
        });
      }
    } catch (error) {
      console.error('解析文件失败:', error);
      Notification.error({
        content: '解析文件失败，请确保文件格式正确',
        position: 'topLeft',
      });
    } finally {
      event.target.value = '';
    }
  };
  reader.readAsArrayBuffer(file);
}

// 链切换
async function chainChange() {
  currentChain.value = chainOptions.value.find(i => i.key === chainValue.value) || null
}

// 监控逻辑
function formatNow() { const d = new Date(); return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,'0')}-${String(d.getDate()).padStart(2,'0')} ${String(d.getHours()).padStart(2,'0')}:${String(d.getMinutes()).padStart(2,'0')}:${String(d.getSeconds()).padStart(2,'0')}` }

async function pollOnce() {
  if (data.value.length === 0 || !chainValue.value) return
  const items = data.value.map(item => ({
    key: item.address, address: item.address, private_key: null,
    plat_balance: null, coin_balance: null, nonce: null, exec_status: '0', error_msg: null, retry_flag: false
  }))
  const params = { chain: chainValue.value, coin_config: { coin_type: 'base', contract_address: null, abi: null }, items, only_coin_config: true, thread_count: 5 }
  try {
    const res = await invoke('query_balances_simple', { params })
    if (!res || !res.items) return
    res.items.forEach(r => {
      const idx = data.value.findIndex(d => d.address === r.address)
      if (idx === -1) return
      const it = data.value[idx]
      let changed = []
      if (r.exec_status === '2') {
        // 比较余额变化（字符串数值）
        const oldBal = it.plat_balance === '' ? null : Number(it.plat_balance)
        const newBal = r.plat_balance === '' || r.plat_balance == null ? null : Number(r.plat_balance)
        if (oldBal !== null && newBal !== null && oldBal !== newBal) changed.push('余额变化')
        if (it.nonce !== '' && r.nonce != null && String(it.nonce) !== String(r.nonce)) changed.push('Nonce变化')
        it.plat_balance = r.plat_balance ?? it.plat_balance
        it.nonce = r.nonce ?? it.nonce
        if (changed.length > 0) {
          it.change_desc = changed.join('、')
          it.last_change_at = formatNow()
        } else {
          it.change_desc = it.change_desc || ''
        }
        it.exec_status = '2'
        it.error_msg = ''
      } else if (r.exec_status === '3') {
        it.error_msg = r.error_msg || '查询失败'
        it.exec_status = '3'
      } else {
        it.exec_status = r.exec_status || '0'
      }
    })
  } catch (e) {
    console.error('pollOnce failed:', e)
  }
}

async function startMonitoring() {
  if (data.value.length === 0) { Notification.warning({ content: '请先导入地址', position: 'topLeft' }); return }
  if (!chainValue.value) { Notification.warning({ content: '请先选择区块链', position: 'topLeft' }); return }
  if (monitoring.value) return
  monitoring.value = true
  showProgress.value = true
  await pollOnce()
  const sec = Number(intervalSec.value) > 0 ? Number(intervalSec.value) : 15
  timer = setInterval(pollOnce, sec * 1000)
  Notification.success({ content: '已开始监控', position: 'topLeft' })
}

function stopMonitoring() {
  if (!monitoring.value) return
  monitoring.value = false
  showProgress.value = false
  if (timer) { clearInterval(timer); timer = null }
  Notification.info({ content: '已停止监控', position: 'topLeft' })
}

function deleteSelected() {
  if (monitoring.value) { Notification.warning({ content: '请先停止监控后再删除', position: 'topLeft' }); return }
  data.value = data.value.filter(i => !selectedKeys.value.includes(i.address))
  Notification.success({ content: '删除成功', position: 'topLeft' })
}

function clearAll() {
  if (monitoring.value) { Notification.warning('请先停止监控后再清空'); return }
  data.value = []
}

// 生命周期
onBeforeMount(async () => {
  const result = await invoke('get_chain_list')
  chainOptions.value = (result || []).filter(item => item.ecosystem === 'evm')
  if (chainOptions.value && chainOptions.value.length > 0) {
    chainValue.value = chainOptions.value[0].key
    currentChain.value = chainOptions.value[0]
  }
})

onMounted(async () => {
  const isTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__;
  if (isTauri) {
    try {
      const currentWindow = getCurrentWindow();
      const title = await currentWindow.title();
      if (title) windowTitle.value = title;
      currentWindow.emit('page-loaded');
    } catch {}
  }
})

const statistics = computed(() => {
  const total = data.value.length
  const succeeded = data.value.filter(i => i.exec_status === '2').length
  const failed = data.value.filter(i => i.exec_status === '3').length
  return { total, succeeded, failed }
})
</script>

<template>
  <TitleBar :title="windowTitle" />
  <div class="container monitor" style="height: 100vh; display: flex; flex-direction: column; overflow: hidden;">
    <div class="toolBar" style="flex-shrink: 0; display:flex; gap:10px; align-items:center; margin-top: 45px;">
      <a-button type="primary" @click="openImport">录入钱包地址</a-button>
      <a-divider direction="vertical" />
      <a-button type="primary" status="success" v-if="!monitoring" @click="startMonitoring">开始监控</a-button>
      <a-button type="primary" status="danger" v-else @click="stopMonitoring">停止监控</a-button>
      <span style="margin-left:8px; color:#86909c;">间隔(秒)</span>
      <a-input-number v-model="intervalSec" :min="5" :max="600" :step="1" style="width: 100px;" />
      <a-divider direction="vertical" />
      <a-button type="outline" status="danger" @click="deleteSelected">删除选中</a-button>
      <a-button type="outline" status="danger" @click="clearAll">清空列表</a-button>
    </div>

    <div style="display:flex; gap:10px; align-items:center; margin: 6px 0; flex-shrink: 0;">
      <a-button type="primary" @click="() => chainManageRef?.show()">区块链管理</a-button>
      <a-button type="primary" @click="() => rpcManageRef?.show()" :disabled="!chainValue">RPC管理</a-button>
      <a-select v-model="chainValue" :options="chainOptions" @change="chainChange" :field-names="chainFieldNames" size="large" :style="{ width: '65%' }">
        <template #label="{ data }">
          <div style="display:flex; align-items:center; width:100%;">
            <span style="color:gray;">区块链：</span>
            <span style="margin-left:10px">{{ data?.name }}</span>
            <span style="margin-left:20px; color:#c3c3c3;">{{ data?.scan_url }}</span>
          </div>
        </template>
      </a-select>
    </div>

    <div class="mainTable" style="flex:1; overflow:hidden; display:flex; flex-direction:column; min-height:0;">
      <VirtualScrollerTable 
        :columns="columns" 
        :data="data" 
        :row-selection="rowSelection" 
        :selected-keys="selectedKeys" 
        row-key="address" 
        height="100%"
        page-type="monitor"
        :empty-data="data.length === 0"
        @open-manual-import="handleManualImport"
        @open-file-upload="handleFileUpload"
        @download-template="downloadTemplate"
      >
        <template #optional="{ record }">
          <a-button type="text" size="small" status="danger" @click.stop="data = data.filter(i => i.address !== record.address)">删除</a-button>
        </template>
      </VirtualScrollerTable>
    </div>

    <div style="display:flex; gap:12px; align-items:center; justify-content:center; margin: 6px 0; flex-shrink: 0;">
      <a-tag>总数：{{ statistics.total }}</a-tag>
      <a-tag color="#00b42a">成功：{{ statistics.succeeded }}</a-tag>
      <a-tag color="#f53f3f">失败：{{ statistics.failed }}</a-tag>
    </div>
  </div>

  <!-- 导入弹窗 -->
  <a-modal v-model:visible="importVisible" :width="700" title="录入钱包地址" @cancel="cancelImport" :on-before-ok="confirmImport" :confirm-loading="importLoading" :cancel-button-props="{ disabled: importLoading }" :mask-closable="!importLoading" :closable="!importLoading">
    <div style="margin-top: 10px; height: 400px;">
      <CodeEditor v-model="importText" :error-lines="addressErrorLines" :disabled="importLoading" placeholder="格式：一行一个地址&#10;示例：0x742d35Cc6634C0532925a3b8D4..." @input="validateImportData" style="height: 100%;" />
      <div v-if="validationErrors.length > 0" style="margin-top: 10px;">
        <a-alert type="error" :title="`发现 ${validationErrors.length} 个问题`" :show-icon="true">
          <ul style="margin: 8px 0 0 0; padding-left: 20px;">
            <li v-for="(err, i) in validationErrors" :key="i" style="margin-bottom: 4px; color: #f53f3f; font-size: 12px;">{{ err }}</li>
          </ul>
        </a-alert>
      </div>
    </div>
  </a-modal>

  <!-- 管理弹窗 -->
  <ChainManagement ref="chainManageRef" ecosystem-filter="evm" />
  <RpcManagement ref="rpcManageRef" :chain-value="chainValue" />

  <!-- 隐藏的文件输入框 -->
  <input
    type="file"
    ref="uploadInputRef"
    accept=".xlsx,.xls,.csv"
    style="display: none"
    @change="handleFileChange"
  />
</template>

<style scoped>
.container { padding: 10px; height: calc(100vh - 30px); display: flex; flex-direction: column; overflow: hidden; background: var(--bg-color, rgb(42, 42, 43)); }
.toolBar { margin-top: 45px; }
.mainTable { margin-top: 10px; height: 100%; display: flex; flex-direction: column; }
</style>
