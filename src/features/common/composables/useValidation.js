import { Notification } from '@arco-design/web-vue'

/**
 * Shared form validation composable for both EVM and Solana transfers.
 * Ecosystem-specific behavior (e.g. Gas Limit vs Priority Fee) is driven by the `ecosystem` option.
 */
export function useValidation(options = {}) {
  const { form, formRef, ecosystem = 'evm' } = options

  const isEVM = ecosystem === 'evm'
  const precisionMax = isEVM ? 18 : 9

  function validateForm() {
    return new Promise((resolve, reject) => {
      const checks = [checkSendType(), checkPrecision(), checkDelay()]
      if (isEVM) {
        checks.push(checkGasLimit(), checkGasPrice())
      } else {
        checks.push(checkPriorityFee())
      }
      if (checks.every(Boolean)) {
        resolve()
      } else {
        reject()
      }
    })
  }

  function checkSendType() {
    if (form.send_type === '1') return true
    if (form.send_type === '2') {
      const valid = /^\d+(\.\d+)?$/.test(form.send_count) && Number(form.send_count) > 0
      if (form.amount_from === '2' && !valid) {
        Notification.error({ content: '发送数量必须为数字且大于0', position: 'topLeft' })
        setFieldError('send_count', '发送数量必须为数字且大于0')
        return false
      }
      return true
    }
    if (form.send_type === '3' || form.send_type === '4') {
      const minValid = /^\d+(\.\d+)?$/.test(form.send_min_count) && Number(form.send_min_count) > 0
      const maxValid = /^\d+(\.\d+)?$/.test(form.send_max_count) && Number(form.send_max_count) > 0
      if (!minValid || !maxValid) {
        const msg = form.send_type === '4' ? '剩余数量必须为数字且大于0' : '发送数量必须为数字且大于0'
        Notification.error({ content: msg, position: 'topLeft' })
        setFieldError('send_count_scope', '数量范围错误')
        return false
      }
      if (Number(form.send_min_count) > Number(form.send_max_count)) {
        const msg = form.send_type === '4'
          ? '最大剩余数量应该大于等于最小剩余数量'
          : '最大发送数量应该大于等于最小发送数量'
        Notification.error({ content: msg, position: 'topLeft' })
        setFieldError('send_count_scope', '数量范围错误')
        return false
      }
      return true
    }
    Notification.error({ content: '发送类型错误', position: 'topLeft' })
    return false
  }

  function checkPrecision() {
    const valid = /^\d+(\.\d+)?$/.test(form.amount_precision) &&
      Number(form.amount_precision) > 0 &&
      Number(form.amount_precision) < precisionMax
    if (!valid) {
      Notification.error({ content: `金额精度必须为数字且大于0小于${precisionMax}`, position: 'topLeft' })
      setFieldError('amount_precision', `应大于0小于${precisionMax}`)
      return false
    }
    return true
  }

  function checkGasPrice() {
    if (form.gas_price_type === '1') return true
    if (form.gas_price_type === '2') {
      const valid = /^\d+(\.\d+)?$/.test(form.gas_price) && Number(form.gas_price) > 0
      if (!valid) {
        Notification.error({ content: 'Gas Price必须为数字且大于0', position: 'topLeft' })
        setFieldError('gas_price', '必须为数字且大于0')
        return false
      }
      return true
    }
    if (form.gas_price_type === '3') {
      const rateValid = /^\d+$/.test(form.gas_price_rate) && Number(form.gas_price_rate) > 0
      if (!rateValid) {
        Notification.error({ content: 'Gas Price 提高比例应为正整数', position: 'topLeft' })
        setFieldError('gas_price_rate', '比例应为正整数')
        return false
      }
      if (form.max_gas_price) {
        const maxValid = /^\d+(\.\d+)?$/.test(form.max_gas_price) && Number(form.max_gas_price) > 0
        if (!maxValid) {
          Notification.error({ content: '最大 Gas Price 设置必须为数字且大于0', position: 'topLeft' })
          setFieldError('max_gas_price', '必须为数字且大于0')
          return false
        }
      }
      return true
    }
    Notification.error({ content: 'Gas Price 方式错误', position: 'topLeft' })
    return false
  }

  function checkGasLimit() {
    if (form.limit_type === '1') return true
    if (form.limit_type === '2') {
      const valid = /^\d+$/.test(form.limit_count) && Number(form.limit_count) > 0
      if (!valid) {
        Notification.error({ content: 'Gas Limit 数量必须为正整数', position: 'topLeft' })
        setFieldError('limit_count', '数量必须为正整数')
        return false
      }
      return true
    }
    if (form.limit_type === '3') {
      const minValid = /^\d+$/.test(form.limit_min_count) && Number(form.limit_min_count) > 0
      const maxValid = /^\d+$/.test(form.limit_max_count) && Number(form.limit_max_count) > 0
      if (!minValid || !maxValid) {
        Notification.error({ content: 'Gas Limit 数量范围必须为正整数', position: 'topLeft' })
        setFieldError('limit_count_scope', '数量范围必须为正整数')
        return false
      }
      if (Number(form.limit_min_count) > Number(form.limit_max_count)) {
        Notification.error({ content: '最大 Gas Limit 数量应该大于等于最小 Gas Limit 数量', position: 'topLeft' })
        setFieldError('limit_count_scope', '范围错误')
        return false
      }
      return true
    }
    Notification.error({ content: 'Gas Limit 类型错误', position: 'topLeft' })
    return false
  }

  function checkPriorityFee() {
    if (form.gas_price_type === '1') return true
    if (form.gas_price_type === '2') {
      const valid = /^\d+$/.test(form.gas_price) && Number(form.gas_price) >= 0
      if (!valid) {
        Notification.error({ content: '优先费必须为非负整数 (MicroLamports)', position: 'topLeft' })
        setFieldError('gas_price', '必须为非负整数')
        return false
      }
      return true
    }
    return true
  }

  function checkDelay() {
    const minValid = form.min_interval === '0' ||
      (/^\d+$/.test(form.min_interval) && Number(form.min_interval) >= 0)
    const maxValid = form.max_interval === '0' ||
      (/^\d+$/.test(form.max_interval) && Number(form.max_interval) >= 0)
    if (!minValid || !maxValid) {
      Notification.error({ content: '发送间隔必须为正整数或者0', position: 'topLeft' })
      setFieldError('interval_scope', '发送间隔必须为正整数或者0')
      return false
    }
    if (Number(form.min_interval) > Number(form.max_interval)) {
      Notification.error({ content: '最大间隔应该大于等于最小间隔', position: 'topLeft' })
      setFieldError('interval_scope', '最大间隔应该大于等于最小间隔')
      return false
    }
    return true
  }

  function setFieldError(field, message) {
    if (formRef?.value && formRef.value.setFields) {
      formRef.value.setFields({ [field]: { status: 'error', message } })
    }
  }

  return {
    validateForm,
    checkSendType,
    checkPrecision,
    checkGasPrice,
    checkGasLimit,
    checkPriorityFee,
    checkDelay,
  }
}
