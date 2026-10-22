import { describe, expect, it } from 'vitest'
import { confidenceWord, rupees, sourceLabel } from '../src/lib/format'

describe('format helpers', () => {
  it('formats rupees the Indian way', () => {
    expect(rupees(1100)).toBe('₹1,100')
    expect(rupees(21000)).toBe('₹21,000')
    expect(rupees(0)).toBe('₹0')
  })

  it('labels every field source', () => {
    expect(sourceLabel('voice')).toBe('you said')
    expect(sourceLabel('vision')).toBe('vision')
    expect(sourceLabel('default')).toBe('default')
  })

  it('turns confidence into a word a volunteer can act on', () => {
    expect(confidenceWord(0.95)).toBe('strong')
    expect(confidenceWord(0.8)).toBe('likely')
    expect(confidenceWord(0.6)).toBe('check this')
    expect(confidenceWord(0.2)).toBe('guess')
  })
})
