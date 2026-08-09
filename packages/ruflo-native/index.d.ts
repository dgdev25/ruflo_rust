export interface EmbeddingResult { dimensions: number; vector: number[]; provider: string }
export interface RouteResult { agent: string; score: number; strategy: string }
export function embed(text: string, dimensions?: number): EmbeddingResult;
export function cosineSimilarity(left: number[], right: number[]): number;
export function route(task: string, candidates: string[]): RouteResult;
