import { useState, useEffect } from 'react'

export default function App() {
  const [status, setStatus] = useState<any>(null)
  const [address, setAddress] = useState('0x0000000000000000000000000000000000000000')
  const [output, setOutput] = useState<string>('нажми кнопку сверху')
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    fetch('/v1/status')
      .then(r => r.json())
      .then(setStatus)
      .catch(e => setStatus({ error: e.message }))
  }, [])

  const callApi = async (url: string) => {
    setLoading(true)
    setOutput('гружу...')
    try {
      const res = await fetch(url)
      const data = await res.json()
      setOutput(JSON.stringify(data, null, 2))
    } catch (err: any) {
      setOutput(`ошибка: ${err.message}`)
    } finally {
      setLoading(false)
    }
  }

  return (
    <div style={{ maxWidth: '800px', margin: '0 auto' }}>

      <div style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        gap: '15px',
        marginBottom: '15px'
      }}>
        <img
          src="https://media.tenor.com/h_jy2s28rlYAAAAj/spinning-cat.gif"
          alt="left chad"
          style={{ width: '48px', height: '48px', objectFit: 'contain' }}
        />
        <h2 style={{ margin: 0 }}>ERC20 DEBUGGER ДЛЯ ПАЦАНОВ</h2>
        <img
          src="https://media.tenor.com/kBAX25HbTYwAAAAj/cat-rotating.gif"
          alt="right chad"
          style={{ width: '48px', height: '48px', objectFit: 'contain' }}
        />
      </div>

      <div style={{ background: '#222', padding: '10px', borderRadius: '4px', marginBottom: '20px' }}>
        <strong>Sync Status:</strong> {status ? JSON.stringify(status) : 'проверяю...'}
      </div>

      <div style={{ marginBottom: '15px' }}>
        <label>ETH / Token Address:</label><br />
        <input
          style={{ width: '100%', padding: '8px', marginTop: '5px', background: '#333', color: '#fff', border: '1px solid #555' }}
          value={address}
          onChange={e => setAddress(e.target.value)}
        />
      </div>

      <div style={{ display: 'flex', gap: '10px', flexWrap: 'wrap', marginBottom: '20px' }}>
        <button onClick={() => callApi(`/v1/tokens/${address}`)}>Мета токена</button>
        <button onClick={() => callApi(`/v1/addresses/${address}/balances`)}>Балансы адреса</button>
        <button onClick={() => callApi(`/v1/tokens/${address}/transfers`)}>Трансферы токена</button>
        <button onClick={() => callApi(`/v1/addresses/${address}/transfers`)}>Трансферы адреса</button>
      </div>

      <pre style={{
        background: '#050505',
        padding: '15px',
        borderRadius: '5px',
        overflowX: 'auto',
        border: '1px solid #333',
        minHeight: '200px'
      }}>
        {loading ? 'запрос летит...' : output}
      </pre>
    </div>
  )
}
