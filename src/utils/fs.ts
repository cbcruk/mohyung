import {
  mkdir,
  readFile,
  writeFile,
  chmod,
  stat,
  readdir,
  rm,
} from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { dirname, join } from 'node:path'

/**
 * 파일이 존재하는지 확인
 */
export function exists(path: string): boolean {
  return existsSync(path)
}

/**
 * 부모 디렉토리 생성 후 파일 쓰기
 */
export async function writeFileWithDir(
  path: string,
  data: Buffer
): Promise<void> {
  const dir = dirname(path)

  await mkdir(dir, { recursive: true })
  await writeFile(path, data)
}

/**
 * 파일 권한 설정
 */
export async function setFileMode(path: string, mode: number): Promise<void> {
  await chmod(path, mode)
}

/**
 * 파일 읽기
 */
export async function readFileBuffer(path: string): Promise<Buffer> {
  return readFile(path)
}

/**
 * 파일 정보 가져오기
 */
export async function getFileStats(path: string) {
  return stat(path)
}

/**
 * 디렉토리 내 모든 항목 가져오기
 */
export async function listDir(path: string) {
  return readdir(path, { withFileTypes: true })
}

/**
 * 디렉토리 생성
 */
export async function ensureDir(path: string): Promise<void> {
  await mkdir(path, { recursive: true })
}

/**
 * 디렉토리 삭제
 */
export async function removeDir(path: string): Promise<void> {
  await rm(path, { recursive: true, force: true })
}

/**
 * 경로 결합
 */
export { join }
